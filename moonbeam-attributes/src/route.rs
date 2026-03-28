use quote::quote;
use syn::{FnArg, ItemFn, PathArguments, Type, parse_macro_input, parse_quote, spanned::Spanned};

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
/// # Arguments
///
/// * `_attr` - The attribute arguments (currently unused).
/// * `item` - The decorated function as a `TokenStream`.
pub(super) fn route_impl(
	_attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let mut input_fn = parse_macro_input!(item as ItemFn);

	let mut state_type: Option<Type> = None;

	// First pass: Identify state type and update lifetimes
	for arg in &mut input_fn.sig.inputs {
		if let FnArg::Typed(pat_type) = arg {
			let ty = &mut pat_type.ty;

			// Check for Request and add lifetimes if needed
			if let Type::Path(type_path) = &mut **ty
				&& let Some(s) = type_path.path.segments.last_mut()
				&& s.ident == "Request"
				&& s.arguments.is_empty()
			{
				s.arguments = PathArguments::AngleBracketed(parse_quote!(<'_, '_>));
			}

			// Check for State reference
			if let Type::Reference(type_ref) = &mut **ty {
				if type_ref.lifetime.is_none() {
					type_ref.lifetime = Some(parse_quote!('static));
				}
				state_type = Some(*type_ref.elem.clone());
			}
		} else {
			return syn::Error::new(arg.span(), "Route handlers cannot take 'self'")
				.to_compile_error()
				.into();
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
			let is_request = if let Type::Path(type_path) = &**ty {
				type_path
					.path
					.segments
					.last()
					.map(|s| s.ident == "Request")
					.unwrap_or(false)
			} else {
				false
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
			} else if let Type::Reference(_) = &**ty {
				extractions.push(quote! {
					let #arg_name = state;
				});
				call_args.push(quote!(#arg_name));
			} else {
				// FromRequest extractor
				extractions.push(quote! {
					let #arg_name = match <#ty as ::moonbeam::http::FromRequest<'a, 'b, #state_ty_path>>::from_request(req, state).await {
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
			fn call<'a, 'b>(&self, req: ::moonbeam::http::Request<'a, 'b>, params: &'_[&'b str], state: &'static #state_ty_path)
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
