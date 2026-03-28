#![cfg_attr(docsrs, feature(doc_cfg))]
//! # Moonbeam Attributes
//!
//! This crate provides procedural macros for the `moonbeam` web server library.
//!
//! These macros are designed to eliminate boilerplate and provide a clean, declarative DSL for
//! building web servers and routing systems.
//!
//! ## Core Macros
//!
//! - `#[server]`: Turns a function into a full `Server` implementation.
//! - `router!`: Defines a routing tree with nesting and middleware.
//! - `#[route]`: Defines a handler for use within a `router!`.
//! - `#[middleware]`: Defines a middleware function for use within a `router!`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
	FnArg, Ident, ItemFn, PathArguments, ReturnType, Type, TypeImplTrait, TypeReference,
	parse::{Parse, ParseStream},
	parse_macro_input, parse_quote,
};

#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
mod middleware;
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
mod route;
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
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
/// This macro handles the boilerplate of implementing the `moonbeam::Server` trait.
/// It generates a struct with the specified name that can be passed to `moonbeam::serve`.
///
/// # Arguments
///
/// * `name` - The identifier for the generated server struct.
///
/// # Supported Signatures
///
/// The decorated function can accept one of two forms:
///
/// 1. **Stateless**: `fn(Request) -> Response` (or `async fn`, or `-> impl Future`)
/// 2. **Stateful**: `fn(Request, &State) -> Response` (requires passing state to the struct)
///
/// # Example: Stateless
/// ```rust,ignore
/// use moonbeam::{Request, Response, server};
///
/// #[server(MyServer)]
/// async fn handle(req: Request) -> Response {
///     Response::ok()
/// }
///
/// // Usage:
/// // moonbeam::serve("127.0.0.1:8080", MyServer);
/// ```
///
/// # Example: Stateful
/// ```rust,ignore
/// use moonbeam::{Request, Response, server};
/// use std::cell::Cell;
///
/// struct AppState { count: Cell<usize> }
///
/// #[server(MyServer)]
/// async fn handle(req: Request, state: &AppState) -> Response {
///     state.count.set(state.count.get() + 1);
///     Response::ok()
/// }
///
/// // Usage:
/// // let state = AppState { count: Cell::new(0) };
/// // moonbeam::serve("127.0.0.1:8080", MyServer(state));
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

/// Defines a route handler for use with the `router!` macro.
///
/// This macro transforms an async function into a type that implements `RouteHandler`.
/// It allows for powerful dependency injection, automatically extracting parameters
/// based on the function signature.
///
/// # Supported Parameters
///
/// - `Request`: The incoming HTTP request.
/// - `&State`: A reference to the application state (must match the state type in `router!`).
/// - `PathParams<T>`: Extracted path parameters (e.g., `PathParams<&str>` or `PathParams<(&str, &str)>`).
/// - **Extractors**: Any type that implements `FromRequest`. This allows for flexible,
///   typed body extraction (e.g., `Json<T>`).
///
/// # Return Types
///
/// The decorated function can return any type that implements `Into<Response>`.
/// Common return types include:
/// - `Response`
/// - `Result<T, E>` (where both `T` and `E` implement `Into<Response>`)
/// - `()` (returns `204 No Content`)
/// - `Body` (returns `200 OK` with the body)
/// - `(Body, &'static str)` (returns `200 OK` with a custom Content-Type)
///
/// # Example
/// ```rust,ignore
/// use moonbeam::{Request, Response, route};
/// use moonbeam::router::PathParams;
///
/// #[route]
/// async fn get_user(
///     req: Request,
///     state: &AppState,
///     PathParams(id): PathParams<&str>
/// ) -> Response {
///     Response::ok().with_body(format!("User {} requested by {}", id, state.app_name), Body::TEXT)
/// }
/// ```
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
	route::route_impl(attr, item)
}

/// Defines a router and its routing tree.
///
/// The `router!` macro provides a clean, nested DSL for configuring how requests
/// should be dispatched to handlers.
///
/// # Syntax
///
/// ```rust,ignore
/// router!(RouterName<StateType> {
///     with global_middleware
///
///     get("/") => index_handler,
///
///     "/api" => {
///         with api_auth_middleware
///         post("/users") => create_user_handler,
///         _ => ! // 404 for unmatched /api/*
///     }
///
///     _ => global_404_handler
/// });
/// ```
///
/// # Special Symbols
///
/// - `_ => !`: Returns a default `404 Not Found` response for the current scope.
/// - `_ => handler`: Uses the specified handler for any unmatched paths in the current scope.
///
/// # Example
/// ```rust,ignore
/// use moonbeam::{Request, Response, router, route};
///
/// #[route]
/// async fn hello() -> Response { Response::ok() }
///
/// router!(MyRouter<AppState> {
///     get("/hello") => hello,
///     _ => !
/// });
///
/// // Usage:
/// // moonbeam::serve("127.0.0.1:8080", MyRouter::new(state));
/// ```
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
#[proc_macro]
pub fn router(item: TokenStream) -> TokenStream {
	router::router_impl(item)
}

/// Defines a middleware function for use in a `router!`.
///
/// Middleware functions wrap the execution of downstream handlers, allowing you
/// to perform pre-processing (like authentication or logging) and post-processing
/// (like adding headers or timing).
///
/// # Signature
///
/// Middleware functions must accept:
/// 1. `req: Request`
/// 2. `state: &State`
/// 3. `next: Next` (a special type representing the rest of the handler chain)
///
/// And return a `Response`.
///
/// # Example
/// ```rust,ignore
/// use moonbeam::{Request, Response, middleware};
///
/// #[middleware]
/// async fn logger(req: Request, state: &AppState, next: Next) -> Response {
///     let start = std::time::Instant::now();
///     let response = next(req).await;
///     println!("{} {} - {:?}", req.method, req.path, start.elapsed());
///     response
/// }
/// ```
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
#[proc_macro_attribute]
pub fn middleware(attr: TokenStream, item: TokenStream) -> TokenStream {
	middleware::middleware_impl(attr, item)
}
