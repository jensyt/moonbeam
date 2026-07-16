use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

pub fn from_request_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
	let item_impl = parse_macro_input!(item as syn::ItemImpl);

	let trait_segment = match &item_impl.trait_ {
		Some((_, path, _)) => path.segments.last().unwrap(),
		None => {
			return syn::Error::new_spanned(
				&item_impl,
				"expected impl of FromBody or FromState trait",
			)
			.to_compile_error()
			.into();
		}
	};

	if trait_segment.ident == "FromBody" {
		let body_lt = match &trait_segment.arguments {
			syn::PathArguments::AngleBracketed(args) => {
				let mut args_iter = args.args.iter();
				match args_iter.next() {
					Some(syn::GenericArgument::Lifetime(lt)) => lt.clone(),
					_ => {
						return syn::Error::new_spanned(
							trait_segment,
							"expected lifetime as argument to FromBody",
						)
						.to_compile_error()
						.into();
					}
				}
			}
			_ => {
				return syn::Error::new_spanned(
					trait_segment,
					"expected angle-bracketed arguments for FromBody",
				)
				.to_compile_error()
				.into();
			}
		};

		let mut generics = item_impl.generics.clone();
		generics.params.push(syn::parse_quote!(__State));

		let (impl_generics, _, where_clause) = generics.split_for_impl();
		let self_ty = &item_impl.self_ty;

		let expanded = quote! {
			#item_impl

			impl #impl_generics ::moonbeam::http::FromRequest<'_, #body_lt, '_, __State>
			for #self_ty
			#where_clause {
				type Error = <#self_ty as ::moonbeam::http::FromBody<#body_lt>>::Error;

				async fn from_request(
					req: ::moonbeam::http::Request<'_, #body_lt>,
					_state: &__State,
				) -> ::std::result::Result<Self, Self::Error> {
					<Self as ::moonbeam::http::FromBody<#body_lt>>::from_body(req.body)
				}
			}
		};

		TokenStream::from(expanded)
	} else if trait_segment.ident == "FromState" {
		let (state_lt, state_ty) = match &trait_segment.arguments {
			syn::PathArguments::AngleBracketed(args) => {
				let mut args_iter = args.args.iter();
				let state_lt = match args_iter.next() {
					Some(syn::GenericArgument::Lifetime(lt)) => lt.clone(),
					_ => {
						return syn::Error::new_spanned(
							trait_segment,
							"expected lifetime as first argument to FromState",
						)
						.to_compile_error()
						.into();
					}
				};
				let state_ty = match args_iter.next() {
					Some(syn::GenericArgument::Type(ty)) => ty.clone(),
					_ => {
						return syn::Error::new_spanned(
							trait_segment,
							"expected state type as second argument to FromState",
						)
						.to_compile_error()
						.into();
					}
				};
				(state_lt, state_ty)
			}
			_ => {
				return syn::Error::new_spanned(
					trait_segment,
					"expected angle-bracketed arguments for FromState",
				)
				.to_compile_error()
				.into();
			}
		};

		let (impl_generics, _, where_clause) = item_impl.generics.split_for_impl();
		let self_ty = &item_impl.self_ty;

		let expanded = quote! {
			#item_impl

			impl #impl_generics ::moonbeam::http::FromRequest<'_, '_, #state_lt, #state_ty>
			for #self_ty
			#where_clause {
				type Error = <#self_ty as ::moonbeam::http::FromState<#state_lt, #state_ty>>::Error;

				async fn from_request(
					_req: ::moonbeam::http::Request<'_, '_>,
					state: &#state_lt #state_ty,
				) -> ::std::result::Result<Self, Self::Error> {
					<Self as ::moonbeam::http::FromState<#state_lt, #state_ty>>::from_state(state)
				}
			}
		};

		TokenStream::from(expanded)
	} else {
		syn::Error::new_spanned(
			trait_segment,
			"expected impl of FromBody or FromState trait",
		)
		.to_compile_error()
		.into()
	}
}
