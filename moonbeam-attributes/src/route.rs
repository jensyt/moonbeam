use quote::quote;
use syn::{
	FnArg, Ident, ItemFn, PathArguments, Token, Type,
	parse::{Parse, ParseStream},
	parse_macro_input, parse_quote,
	spanned::Spanned,
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

	let mut spawner_lt: Option<syn::Lifetime> = None;
	let mut state_lt: Option<syn::Lifetime> = None;

	// First pass: Identify state type and update/extract lifetimes
	for arg in &mut input_fn.sig.inputs {
		if let FnArg::Typed(pat_type) = arg {
			let ty = &mut pat_type.ty;

			// Check for Request and Spawner and add/extract lifetimes if needed
			if let Type::Path(type_path) = &mut **ty
				&& let Some(s) = type_path.path.segments.last_mut()
			{
				if s.ident == "Request" && s.arguments.is_empty() {
					s.arguments = PathArguments::AngleBracketed(parse_quote!(<'_, '_>));
				} else if s.ident == "Spawner" {
					let mut has_lifetime = false;
					if let PathArguments::AngleBracketed(ab) = &s.arguments {
						for g_arg in &ab.args {
							if let syn::GenericArgument::Lifetime(lt) = g_arg {
								if lt.ident != "_" {
									spawner_lt = Some(lt.clone());
									has_lifetime = true;
								}
								break;
							}
						}
					}
					if !has_lifetime {
						s.arguments = PathArguments::AngleBracketed(parse_quote!(<'e>));
						spawner_lt = Some(parse_quote!('e));
					}
				}
			}

			// Check for State reference
			if let Type::Reference(type_ref) = &mut **ty {
				if state_type.is_none() {
					state_type = Some(*type_ref.elem.clone());
				}
				let mut has_lifetime = false;
				if let Some(lt) = &type_ref.lifetime
					&& lt.ident != "_"
				{
					state_lt = Some(lt.clone());
					has_lifetime = true;
				}
				if !has_lifetime {
					let lt: syn::Lifetime = parse_quote!('s);
					type_ref.lifetime = Some(lt.clone());
					state_lt = Some(lt);
				}
			}
		} else {
			return syn::Error::new(arg.span(), "Route handlers cannot take 'self'")
				.to_compile_error()
				.into();
		}
	}

	let mut existing_lifetimes = std::collections::HashSet::new();
	for param in &input_fn.sig.generics.params {
		if let syn::GenericParam::Lifetime(lt) = param {
			existing_lifetimes.insert(lt.lifetime.ident.to_string());
		}
	}

	if let Some(ref lt) = spawner_lt
		&& !existing_lifetimes.contains(&lt.ident.to_string())
	{
		input_fn
			.sig
			.generics
			.params
			.push(syn::GenericParam::Lifetime(syn::LifetimeParam::new(
				lt.clone(),
			)));
	}

	if let Some(ref lt_s) = state_lt
		&& !existing_lifetimes.contains(&lt_s.ident.to_string())
	{
		let mut lt_param = syn::LifetimeParam::new(lt_s.clone());
		if let Some(ref lt_e) = spawner_lt {
			lt_param.bounds.push(lt_e.clone());
		}
		input_fn
			.sig
			.generics
			.params
			.push(syn::GenericParam::Lifetime(lt_param));
	}

	if let (Some(lt_s), Some(lt_e)) = (&state_lt, &spawner_lt) {
		for param in &mut input_fn.sig.generics.params {
			if let syn::GenericParam::Lifetime(lt_param) = param
				&& lt_param.lifetime.ident == lt_s.ident
			{
				if !lt_param.bounds.iter().any(|b| b.ident == lt_e.ident) {
					lt_param.bounds.push(lt_e.clone());
				}
				break;
			}
		}
	}

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
				// FromRequest extractor
				extractions.push(quote! {
					let #arg_name = match <#ty as ::moonbeam::http::FromRequest<'a, 'b, 's, #state_ty_path>>::from_request(req, state).await {
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
			fn call<'a, 'b, 's: 'e, 'e>(&self, req: ::moonbeam::http::Request<'a, 'b>, params: &[&'b str], spawner: ::moonbeam::server::task::Spawner<'e>, state: &'s #state_ty_path)
				-> impl ::std::future::Future<Output = ::moonbeam::http::Response>
			{
				async move {
					#(#extractions)*
					let resp = #handler_call;
					::core::convert::Into::into(resp)
				}
			}
		}
	};

	output.into()
}
