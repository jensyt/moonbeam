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
	spanned::Spanned,
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
/// 1. **Stateless**: `fn(Request, Spawner) -> Response`
/// 2. **Stateful**: `fn(Request, Spawner, &State) -> Response` (requires passing state to the struct)
///
/// # Example: Stateless
/// ```rust,ignore
/// use moonbeam::{Request, Response, Spawner, server};
///
/// #[server(MyServer)]
/// async fn handle(req: Request, _spawner: Spawner<'_>) -> Response {
///     Response::ok()
/// }
///
/// // Usage:
/// // moonbeam::serve("127.0.0.1:8080", || MyServer);
/// ```
///
/// # Example: Stateful
/// ```rust,ignore
/// use moonbeam::{Request, Response, Spawner, server};
/// use std::cell::Cell;
///
/// struct AppState { count: Cell<usize> }
///
/// #[server(MyServer)]
/// async fn handle(req: Request, _spawner: Spawner<'_>, state: &AppState) -> Response {
///     state.count.set(state.count.get() + 1);
///     Response::ok()
/// }
///
/// // Usage:
/// // let state = AppState { count: Cell::new(0) };
/// // moonbeam::serve("127.0.0.1:8080", move || MyServer(state));
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
		let span = sig
			.inputs
			.first()
			.map(|f| f.span())
			.unwrap_or_else(|| sig.inputs.span());
		return syn::Error::new(span, "First parameter must be Request")
			.to_compile_error()
			.into();
	}

	let has_state = sig.inputs.len() > 2;
	let spawner_lt = match check_spawner(sig.inputs.iter_mut().nth(1), has_state) {
		Some(lt) => lt,
		None => {
			let span = sig
				.inputs
				.iter()
				.nth(1)
				.map(|f| f.span())
				.unwrap_or_else(|| sig.inputs.span());
			return syn::Error::new(span, "Second parameter must be Spawner")
				.to_compile_error()
				.into();
		}
	};

	let third_param = if has_state {
		match check_state(sig.inputs.iter_mut().nth(2)) {
			Some(lt) => {
				// Inject lifetimes dynamically into generics
				let mut existing_lifetimes = std::collections::HashSet::new();
				for param in &input_fn.sig.generics.params {
					if let syn::GenericParam::Lifetime(g_lt) = param {
						existing_lifetimes.insert(g_lt.lifetime.ident.to_string());
					}
				}

				if let Some(ref lt_e) = spawner_lt
					&& !existing_lifetimes.contains(&lt_e.ident.to_string())
				{
					input_fn
						.sig
						.generics
						.params
						.push(syn::GenericParam::Lifetime(syn::LifetimeParam::new(
							lt_e.clone(),
						)));
				}

				if !existing_lifetimes.contains(&lt.ident.to_string()) {
					let mut lt_param = syn::LifetimeParam::new(lt.clone());
					if let Some(ref lt_e) = spawner_lt {
						lt_param.bounds.push(lt_e.clone());
					}
					input_fn
						.sig
						.generics
						.params
						.push(syn::GenericParam::Lifetime(lt_param));
				}

				if let Some(ref lt_e) = spawner_lt {
					for param in &mut input_fn.sig.generics.params {
						if let syn::GenericParam::Lifetime(lt_param) = param
							&& lt_param.lifetime.ident == lt.ident
						{
							if !lt_param.bounds.iter().any(|b| b.ident == lt_e.ident) {
								lt_param.bounds.push(lt_e.clone());
							}
							break;
						}
					}
				}

				Some(get_state(input_fn.sig.inputs.iter().nth(2)))
			}
			None => {
				let span = sig
					.inputs
					.iter()
					.nth(2)
					.map(|f| f.span())
					.unwrap_or_else(|| sig.inputs.span());
				return syn::Error::new(span, "Third parameter must be: state: &State")
					.to_compile_error()
					.into();
			}
		}
	} else {
		None
	};

	let fn_name = &input_fn.sig.ident;

	let struct_impl = if let Some(ref_type) = third_param {
		// State type
		let elem = &ref_type.elem;

		quote! {
			#[repr(transparent)]
			pub(crate) struct #wrapper_name(#elem);
		}
	} else {
		quote! {
			pub(crate) struct #wrapper_name;
		}
	};

	let fn_impl = if is_async {
		if has_state {
			quote! { #fn_name(request, spawner, &self.0) }
		} else {
			quote! { #fn_name(request, spawner) }
		}
	} else {
		if has_state {
			quote! { async move { #fn_name(request, spawner, &self.0) } }
		} else {
			quote! { async move { #fn_name(request, spawner) } }
		}
	};

	// Generate the output
	let output = quote! {
		#input_fn

		#struct_impl

		impl ::moonbeam::Server for #wrapper_name {
			#[inline(always)]
			fn route<'s: 'e, 'e>(&'s self, request: ::moonbeam::http::Request<'_, '_>, spawner: ::moonbeam::server::task::Spawner<'e>)
				-> impl ::core::future::Future<Output = ::moonbeam::http::Response>
			{
				#fn_impl
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

fn check_spawner(arg: Option<&mut FnArg>, has_state: bool) -> Option<Option<syn::Lifetime>> {
	if let FnArg::Typed(pat_type) = arg?
		&& let Type::Path(type_path) = &mut *pat_type.ty
		&& let segment = type_path.path.segments.last_mut()?
		&& segment.ident == "Spawner"
	{
		let mut spawner_lt = None;
		let mut has_lifetime = false;
		if let PathArguments::AngleBracketed(ab) = &segment.arguments {
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
		if !has_lifetime && has_state {
			segment.arguments = PathArguments::AngleBracketed(parse_quote!(<'e>));
			spawner_lt = Some(parse_quote!('e));
		} else if !has_lifetime {
			segment.arguments = PathArguments::AngleBracketed(parse_quote!(<'_>));
		}
		Some(spawner_lt)
	} else {
		None
	}
}

fn check_state(arg: Option<&mut FnArg>) -> Option<syn::Lifetime> {
	if let FnArg::Typed(pat_type) = arg?
		&& let Type::Reference(type_ref) = &mut *pat_type.ty
	{
		let mut state_lt = None;
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
		Some(state_lt.unwrap_or_else(|| parse_quote!('s)))
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
///
/// # Example with Explicit State
/// ```rust,ignore
/// #[route(state = AppState)]
/// async fn get_user(PathParams(id): PathParams<&str>) -> Response {
///     // ...
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
/// // moonbeam::serve("127.0.0.1:8080", move || MyRouter::new(state));
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
/// use moonbeam::{Request, Response, middleware, Spawner};
///
/// #[middleware]
/// async fn logger(req: Request, spawner: Spawner, state: &AppState, next: Next) -> Response {
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
