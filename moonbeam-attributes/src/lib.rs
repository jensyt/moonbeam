//! # Moonbeam Attributes
//!
//! This crate provides procedural macros for the `moonbeam` web server library.
//! The main macro is `#[server]`, which simplifies creating server implementations
//! by wrapping a function.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
	FnArg, Ident, ItemFn, ReturnType, Type, TypeImplTrait, parse::Parse, parse::ParseStream,
	parse_macro_input,
};

// Parse the attribute arguments
struct ServerArgs {
	name: Ident,
}

impl Parse for ServerArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let name: Ident = input.parse()?;
		Ok(ServerArgs { name })
	}
}

/// Attribute macro to convert a function into a `Server` implementation.
///
/// This macro simplifies creating a server by handling the boilerplate of implementing the `Server` trait.
/// It wraps the decorated function in a struct that implements `Server`.
///
/// # Arguments
/// * `name` - The name of the struct to generate.
///
/// # Function Signature
/// The decorated function must have one of the following signatures:
/// - `fn(Request) -> impl Future<Output = Response>`
/// - `fn(Request, &State) -> impl Future<Output = Response>` (if state is used)
///
/// The function can be `async` or return `impl Future`.
///
/// # Example
/// ```rust,ignore
/// use moonbeam::{Request, Response, server};
///
/// #[server(MyServer)]
/// async fn handle_request(req: Request<'_, '_>) -> Response {
///     Response::ok().with_body("Hello World!", None)
/// }
///
/// // Usage:
/// // serve("127.0.0.1:8080", MyServer);
/// ```
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as ServerArgs);
	let input_fn = parse_macro_input!(item as ItemFn);

	let wrapper_name = args.name;
	let sig = &input_fn.sig;
	let fn_name = &sig.ident;

	// Check if return type is async or impl Future
	let is_async = sig.asyncness.is_some()
		|| match &sig.output {
			ReturnType::Type(_, ty) => is_impl_future(ty),
			ReturnType::Default => false,
		};

	// Extract first parameter (request)
	match sig.inputs.first() {
		Some(FnArg::Typed(_)) => (),
		_ => {
			return syn::Error::new_spanned(
				&sig.inputs.first(),
				"First parameter must be Request<'_, '_>",
			)
			.to_compile_error()
			.into();
		}
	};

	// Extract second parameter (state)
	let second_param = if sig.inputs.len() > 1 {
		match sig.inputs.iter().nth(1) {
			Some(FnArg::Typed(pat_type)) => Some(pat_type),
			_ => {
				return syn::Error::new_spanned(
					&sig.inputs,
					"Second parameter must be: state: &'static State",
				)
				.to_compile_error()
				.into();
			}
		}
	} else {
		None
	};

	let output = if let Some(second_param) = second_param {
		let state_type = &second_param.ty;

		// Extract the State type from &'static State
		let inner_state_type = extract_static_ref_type(state_type);

		// Generate the route implementation based on async status
		let route_impl = if is_async {
			// Case 1: async fn
			quote! {
				#[inline(always)]
				fn route(&'static self, request: ::moonbeam::http::Request<'_, '_>)
					-> impl ::core::future::Future<Output = ::moonbeam::http::Response>
				{
					#fn_name(request, &self.0)
				}
			}
		} else {
			// Case 2: regular fn - wrap in async block
			quote! {
				#[inline(always)]
				fn route(&'static self, request: ::moonbeam::http::Request<'_, '_>)
					-> impl ::core::future::Future<Output = ::moonbeam::http::Response>
				{
					async move { #fn_name(request, &self.0) }
				}
			}
		};

		// Generate the output
		quote! {
			#input_fn

			#[repr(transparent)]
			pub(crate) struct #wrapper_name(#inner_state_type);

			impl ::moonbeam::Server for #wrapper_name {
				#route_impl
			}
		}
	} else {
		// Generate the route implementation based on async status
		let route_impl = if is_async {
			// Case 1: async fn
			quote! {
				#[inline(always)]
				fn route(&'static self, request: ::moonbeam::http::Request<'_, '_>)
					-> impl ::core::future::Future<Output = ::moonbeam::http::Response>
				{
					#fn_name(request)
				}
			}
		} else {
			// Case 2: regular fn - wrap in async block
			quote! {
				#[inline(always)]
				fn route(&'static self, request: ::moonbeam::http::Request<'_, '_>)
					-> impl ::core::future::Future<Output = ::moonbeam::http::Response>
				{
					async move { #fn_name(request) }
				}
			}
		};

		// Generate the output
		quote! {
			#input_fn

			pub(crate) struct #wrapper_name;

			impl ::moonbeam::Server for #wrapper_name {
				#route_impl
			}
		}
	};

	output.into()
}

// Helper function to check if a type is impl Future
fn is_impl_future(ty: &Type) -> bool {
	match ty {
		Type::ImplTrait(TypeImplTrait { bounds, .. }) => bounds.iter().any(|bound| {
			if let syn::TypeParamBound::Trait(trait_bound) = bound {
				trait_bound
					.path
					.segments
					.last()
					.map(|seg| seg.ident == "Future")
					.unwrap_or(false)
			} else {
				false
			}
		}),
		_ => false,
	}
}

// Helper function to extract State from &'static State
fn extract_static_ref_type(ty: &Type) -> proc_macro2::TokenStream {
	match ty {
		Type::Reference(type_ref) => {
			let elem = &type_ref.elem;
			quote! { #elem }
		}
		_ => {
			// Fallback if it's not a reference type
			quote! { #ty }
		}
	}
}
