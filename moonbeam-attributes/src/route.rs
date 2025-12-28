use quote::quote;
use syn::{FnArg, ItemFn, Type, parse_macro_input, spanned::Spanned};

pub fn route_impl(
	_attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let input_fn = parse_macro_input!(item as ItemFn);
	let vis = &input_fn.vis;
	let sig = &input_fn.sig;
	let fn_name = &sig.ident;
	let inputs = &sig.inputs;

	let mut params_extraction = Vec::new();
	let mut call_args = Vec::new();
	let mut state_type: Option<Type> = None;

	for arg in inputs {
		match arg {
			FnArg::Typed(pat_type) => {
				let ty = &pat_type.ty;

				// Check for PathParams
				let is_path_params = if let Type::Path(type_path) = &**ty {
					type_path
						.path
						.segments
						.last()
						.map(|s| s.ident.to_string().contains("PathParams"))
						.unwrap_or(false)
				} else {
					false
				};

				// Check for State reference: &'static State
				let is_state = if let Type::Reference(type_ref) = &**ty {
					if type_ref
						.lifetime
						.as_ref()
						.map(|l| l.ident == "static")
						.unwrap_or(false)
					{
						state_type = Some(*type_ref.elem.clone());
						true
					} else {
						false
					}
				} else {
					false
				};

				if is_path_params {
					params_extraction.push(quote! {
						let arg_params = <#ty as ::moonbeam::router::FromParams>::from_params(
							params
						);
					});
					call_args.push(quote!(arg_params));
				} else if is_state {
					call_args.push(quote!(state));
				} else {
					// Default to request
					call_args.push(quote!(req));
				}
			}
			FnArg::Receiver(_) => {
				return syn::Error::new(arg.span(), "Route handlers cannot take 'self'")
					.to_compile_error()
					.into();
			}
		}
	}

	let is_async = sig.asyncness.is_some();
	let call_expr = if is_async {
		quote! { Self::#fn_name(#(#call_args),*).await }
	} else {
		quote! { Self::#fn_name(#(#call_args),*) }
	};

	let (impl_generics, state_ty_path) = if let Some(st) = state_type {
		(quote! {}, quote! { #st })
	} else {
		(quote! { <S> }, quote! { S })
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
				#(#params_extraction)*

				async move {
					#call_expr
				}
			}
		}
	};

	output.into()
}
