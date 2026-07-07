use quote::quote;
use syn::{
	FnArg, Ident, ItemFn, Token, Type,
	parse::{Parse, ParseStream},
	parse_macro_input, parse_quote,
	spanned::Spanned,
	visit_mut::VisitMut,
};

/// Arguments for the `#[route]` attribute macro.
struct RouteArgs {
	state: Option<Type>,
}

impl Parse for RouteArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut state = None;
		while !input.is_empty() {
			let ident: Ident = input.parse()?;
			if ident == "state" {
				input.parse::<Token![=]>()?;
				state = Some(input.parse()?);
			} else {
				return Err(syn::Error::new(ident.span(), "expected `state`"));
			}

			if !input.is_empty() {
				input.parse::<Token![,]>()?;
			}
		}
		Ok(RouteArgs { state })
	}
}

/// Implementation logic for the `#[route]` attribute macro.
///
/// This function parses the decorated function and generates a struct that implements `RouteHandler`.
/// It handles:
/// - Parsing function arguments to determine what needs to be injected (Request, PathParams, State).
/// - Generating the `RouteHandler::call` implementation.
/// - Wrapping the user's function call with the extracted arguments.
///
/// # Example
///
/// ```ignore
/// #[route]
/// async fn get_user(req: Request, PathParams(id): PathParams<&str>) -> Response {
///     // ...
/// }
/// ```
///
/// Explicitly specifying state type:
/// ```ignore
/// #[route(state = AppState)]
/// async fn get_user(PathParams(id): PathParams<&str>) -> Response {
///     // ...
/// }
/// ```
///
/// # Arguments
///
/// * `attr` - The attribute arguments.
/// * `item` - The decorated function as a `TokenStream`.
pub(super) fn route_impl(
	attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let args = parse_macro_input!(attr as RouteArgs);
	let mut input_fn = parse_macro_input!(item as ItemFn);

	let mut state_type: Option<Type> = args.state;

	let mut request_lt: Option<syn::Lifetime> = None;
	let mut spawner_lt: Option<syn::Lifetime> = None;
	let mut state_lt: Option<syn::Lifetime> = None;

	// First pass: Identify state type and update/extract lifetimes
	for arg in &mut input_fn.sig.inputs {
		if request_lt.is_none() {
			request_lt = super::check_arg(
				Some(arg),
				"Request",
				parse_quote!('req),
				parse_quote!(<'req, 'req>),
			);
		}

		if spawner_lt.is_none() {
			spawner_lt = super::check_arg(
				Some(arg),
				"Spawner",
				parse_quote!('exec),
				parse_quote!(<'exec>),
			);
		}

		// Check for State reference
		if let FnArg::Typed(pat_type) = arg
			&& let Type::Reference(type_ref) = &mut *pat_type.ty
		{
			if state_type.is_none() {
				state_type = Some(*type_ref.elem.clone());
			} else {
				return syn::Error::new(
					arg.span(),
					"Route handlers cannot take multiple state objects",
				)
				.to_compile_error()
				.into();
			}
			let mut has_lifetime = false;
			if let Some(lt) = &type_ref.lifetime
				&& lt.ident != "_"
			{
				state_lt = Some(lt.clone());
				has_lifetime = true;
			}
			if !has_lifetime {
				let lt: syn::Lifetime = parse_quote!('state);
				type_ref.lifetime = Some(lt.clone());
				state_lt = Some(lt);
			}
		}
	}

	let mut existing_lifetimes = std::collections::HashSet::new();
	for param in &input_fn.sig.generics.params {
		if let syn::GenericParam::Lifetime(lt) = param {
			existing_lifetimes.insert(lt.lifetime.ident.to_string());
		}
	}

	if let Some(ref lt) = request_lt {
		super::add_lifetime_generic(&mut existing_lifetimes, &mut input_fn, lt, None);
	}

	if let Some(ref lt) = spawner_lt {
		super::add_lifetime_generic(
			&mut existing_lifetimes,
			&mut input_fn,
			lt,
			request_lt.as_ref(),
		);
	}

	if let Some(ref lt) = state_lt {
		super::add_lifetime_generic(
			&mut existing_lifetimes,
			&mut input_fn,
			lt,
			spawner_lt.as_ref(),
		);
	}

	let lt = request_lt
		.unwrap_or_else(|| spawner_lt.unwrap_or_else(|| state_lt.unwrap_or(parse_quote!('static))));
	ResponseLifetimeReplacer(lt).visit_return_type_mut(&mut input_fn.sig.output);

	let (impl_generics, state_ty_path) = if let Some(st) = state_type {
		(quote! {}, quote! { #st })
	} else {
		(quote! { <S> }, quote! { S })
	};

	let mut extractions = Vec::new();
	let mut call_args = Vec::new();

	// Second pass: Generate extraction logic
	for (i, arg) in input_fn.sig.inputs.iter().enumerate() {
		if let FnArg::Typed(pat_type) = arg {
			let ty = &pat_type.ty;
			let arg_name = quote::format_ident!("__arg_{}", i);

			// Check for PathParams
			let is_path_params = if let Type::Path(type_path) = &**ty {
				type_path
					.path
					.segments
					.last()
					.map(|s| s.ident == "PathParams")
					.unwrap_or(false)
			} else {
				false
			};

			// Check for Request
			let (is_request, is_spawner) = if let Type::Path(type_path) = &**ty {
				(
					type_path
						.path
						.segments
						.last()
						.map(|s| s.ident == "Request")
						.unwrap_or(false),
					type_path
						.path
						.segments
						.last()
						.map(|s| s.ident == "Spawner")
						.unwrap_or(false),
				)
			} else {
				(false, false)
			};

			if is_path_params {
				extractions.push(quote! {
					let #arg_name = <#ty as ::moonbeam::router::FromParams>::from_params(
						params
					);
				});
				call_args.push(quote!(#arg_name));
			} else if is_request {
				extractions.push(quote! {
					let #arg_name = req;
				});
				call_args.push(quote!(#arg_name));
			} else if is_spawner {
				extractions.push(quote! {
					let #arg_name = spawner;
				});
				call_args.push(quote!(#arg_name));
			} else if let Type::Reference(_) = &**ty {
				extractions.push(quote! {
					let #arg_name = state;
				});
				call_args.push(quote!(#arg_name));
			} else {
				let mut ty_anon = ty.clone();
				AnonymousLifetimeReplacer.visit_type_mut(&mut ty_anon);

				// FromRequest extractor
				extractions.push(quote! {
					let #arg_name =
						match <#ty_anon as ::moonbeam::http::FromRequest<'req, 'req, 'exec, #state_ty_path>>
							::from_request(req, state).await
					{
						Ok(v) => v,
						Err(e) => return ::core::convert::Into::into(e),
					};
				});
				call_args.push(quote!(#arg_name));
			}
		}
	}

	let vis = &input_fn.vis;
	let sig = &input_fn.sig;
	let fn_name = &sig.ident;

	let is_async = sig.asyncness.is_some();
	let handler_call = if is_async {
		quote! { Self::#fn_name(#(#call_args),*).await }
	} else {
		quote! { Self::#fn_name(#(#call_args),*) }
	};

	let output = quote! {
		#[allow(non_camel_case_types)]
		#vis struct #fn_name;

		impl #fn_name {
			#input_fn
		}

		impl #impl_generics ::moonbeam::router::RouteHandler<#state_ty_path> for #fn_name {
			fn call<'req, 'exec: 'req>(
				&self,
				req: ::moonbeam::http::Request<'req, 'req>,
				params: &[&str],
				spawner: ::moonbeam::server::task::Spawner<'exec>,
				state: &'exec #state_ty_path)
			-> impl ::std::future::Future<Output = ::moonbeam::http::Response<'req>>
			{
				::moonbeam::tracing::Instrument::instrument(
					async move {
						#(#extractions)*
						let resp = #handler_call;
						::core::convert::Into::into(resp)
					},
					::moonbeam::tracing::info_span!("handler", name = stringify!(#fn_name))
				)
			}
		}
	};

	output.into()
}

struct AnonymousLifetimeReplacer;
impl VisitMut for AnonymousLifetimeReplacer {
	fn visit_lifetime_mut(&mut self, i: &mut syn::Lifetime) {
		if i.ident != "static" {
			i.ident = syn::Ident::new("_", i.ident.span());
		}
	}
}

struct ResponseLifetimeReplacer(syn::Lifetime);
impl VisitMut for ResponseLifetimeReplacer {
	fn visit_path_mut(&mut self, path: &mut syn::Path) {
		if let Some(segment) = path.segments.last_mut()
			&& segment.ident == "Response"
		{
			let mut has_lifetime = false;
			if let syn::PathArguments::AngleBracketed(ab) = &segment.arguments {
				for g_arg in &ab.args {
					if let syn::GenericArgument::Lifetime(lt) = g_arg {
						if lt.ident != "_" {
							has_lifetime = true;
						}
						break;
					}
				}
			}
			if !has_lifetime {
				let lt = &self.0;
				segment.arguments = syn::PathArguments::AngleBracketed(parse_quote!(<#lt>));
			}
		}

		syn::visit_mut::visit_path_mut(self, path);
	}
}
