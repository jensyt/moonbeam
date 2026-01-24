//! # Moonbeam Attributes
//!
//! This crate provides procedural macros for the `moonbeam` web server library.
//! The main macro is `#[server]`, which simplifies creating server implementations
//! by wrapping a function.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
	FnArg, Ident, ItemFn, PathArguments, ReturnType, Type, TypeImplTrait, TypeReference,
	parse::{Parse, ParseStream},
	parse_macro_input, parse_quote,
};

#[cfg(feature = "router")]
mod middleware;
#[cfg(feature = "router")]
mod route;
#[cfg(feature = "router")]
mod router;

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
/// async fn handle_request(req: Request) -> Response {
///     Response::ok().with_body("Hello World!", None)
/// }
///
/// // Usage:
/// // serve("127.0.0.1:8080", MyServer);
/// ```
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as ServerArgs);
	let mut input_fn = parse_macro_input!(item as ItemFn);

	let wrapper_name = args.name;
	let sig = &mut input_fn.sig;

	// Check if return type is async or impl Future
	let is_async = sig.asyncness.is_some()
		|| match &sig.output {
			ReturnType::Type(_, ty) => is_impl_future(ty),
			ReturnType::Default => false,
		};

	if check_request(sig.inputs.first_mut()).is_none() {
		return syn::Error::new_spanned(&input_fn.sig.inputs, "First parameter must be Request")
			.to_compile_error()
			.into();
	}

	let second_param = if sig.inputs.len() > 1 {
		if check_state(sig.inputs.iter_mut().nth(1)).is_none() {
			return syn::Error::new_spanned(
				&sig.inputs,
				"Second parameter must be: state: &'static State",
			)
			.to_compile_error()
			.into();
		} else {
			Some(get_state(input_fn.sig.inputs.iter().nth(1)))
		}
	} else {
		None
	};

	let fn_name = &input_fn.sig.ident;

	let output = if let Some(ref_type) = second_param {
		// State type
		let elem = &ref_type.elem;

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
			pub(crate) struct #wrapper_name(#elem);

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

fn check_request(arg: Option<&mut FnArg>) -> Option<()> {
	if let FnArg::Typed(pat_type) = arg?
		&& let Type::Path(type_path) = &mut *pat_type.ty
		&& let segment = type_path.path.segments.last_mut()?
		&& segment.ident == "Request"
	{
		if segment.arguments.is_empty() {
			// Inject default lifetime parameters if needed
			segment.arguments = PathArguments::AngleBracketed(parse_quote!(<'_, '_>));
		}
		Some(())
	} else {
		None
	}
}

fn check_state(arg: Option<&mut FnArg>) -> Option<()> {
	if let FnArg::Typed(pat_type) = arg?
		&& let Type::Reference(type_ref) = &mut *pat_type.ty
	{
		if type_ref.lifetime.is_none() {
			// Inject static lifetime if needed
			type_ref.lifetime = Some(parse_quote!('static));
		}
		Some(())
	} else {
		None
	}
}

fn get_state(arg: Option<&FnArg>) -> &TypeReference {
	if let Some(FnArg::Typed(pat_type)) = arg
		&& let Type::Reference(type_ref) = &*pat_type.ty
	{
		type_ref
	} else {
		unreachable!("call to check_state first should ensure this never happens")
	}
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

/// Defines a route handler.
#[cfg(feature = "router")]
#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
	route::route_impl(attr, item)
}

/// Defines a router and its routes.
#[cfg(feature = "router")]
#[proc_macro]
pub fn router(item: TokenStream) -> TokenStream {
	router::router_impl(item)
}

/// Simplifies middleware signature.
#[cfg(feature = "router")]
#[proc_macro_attribute]
pub fn middleware(attr: TokenStream, item: TokenStream) -> TokenStream {
	middleware::middleware_impl(attr, item)
}
