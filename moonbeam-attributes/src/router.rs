use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;
use syn::{
	Ident, LitStr, Path, Token, Type, Visibility,
	parse::{Parse, ParseStream},
	parse_macro_input,
};

struct RouterInput {
	visibility: Visibility,
	name: Ident,
	state_type: Option<Type>,
	items: Vec<RouterItem>,
}

enum RouterItem {
	Route(RouteEntry),
	Group(RouteGroup),
	Middleware(MiddlewareEntry),
}

struct RouteGroup {
	prefix: LitStr,
	_fat_arrow: Token![=>],
	items: Vec<RouterItem>,
}

struct RouteEntry {
	method: Ident,
	path: LitStr,
	middlewares: Vec<Path>,
	_fat_arrow: Token![=>],
	handler: Path,
	_comma: Option<Token![,]>,
}

struct MiddlewareEntry {
	_with: Ident,
	middleware: Path,
	_comma: Option<Token![,]>,
}

impl Parse for RouterInput {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let visibility: Visibility = input.parse()?;
		let name: Ident = input.parse()?;

		let state_type = if input.peek(Token![<]) {
			let _lt: Token![<] = input.parse()?;
			let ty: Type = input.parse()?;
			let _gt: Token![>] = input.parse()?;
			Some(ty)
		} else {
			None
		};

		let content;
		syn::braced!(content in input);

		let mut items = Vec::new();
		while !content.is_empty() {
			items.push(content.parse()?);
		}

		Ok(RouterInput {
			visibility,
			name,
			state_type,
			items,
		})
	}
}

impl Parse for RouterItem {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		if input.peek(LitStr) {
			Ok(RouterItem::Group(input.parse()?))
		} else if input.peek(Ident) {
			let ident: Ident = input.parse()?;
			if ident == "with" {
				Ok(RouterItem::Middleware(MiddlewareEntry {
					_with: ident,
					middleware: input.parse()?,
					_comma: input.parse().ok(),
				}))
			} else {
				Ok(RouterItem::Route(RouteEntry::parse_with_method(
					input, ident,
				)?))
			}
		} else {
			Err(input.error("expected route group, route, or middleware"))
		}
	}
}

impl RouteGroup {
	fn parse_with_prefix(input: ParseStream, prefix: LitStr) -> syn::Result<Self> {
		let fat_arrow: Token![=>] = input.parse()?;

		let content;
		syn::braced!(content in input);

		let mut items = Vec::new();
		while !content.is_empty() {
			items.push(content.parse()?);
		}

		Ok(RouteGroup {
			prefix,
			_fat_arrow: fat_arrow,
			items,
		})
	}
}

impl Parse for RouteGroup {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let prefix: LitStr = input.parse()?;
		Self::parse_with_prefix(input, prefix)
	}
}

impl RouteEntry {
	fn parse_with_method(input: ParseStream, method: Ident) -> syn::Result<Self> {
		let content;
		syn::parenthesized!(content in input);
		let path: LitStr = content.parse()?;

		let mut middlewares = Vec::new();
		while input.peek(Ident) {
			let fork = input.fork();
			let id: Ident = fork.parse()?;
			if id == "with" {
				input.parse::<Ident>()?; // consume "with"
				middlewares.push(input.parse()?);
			} else {
				break;
			}
		}

		let fat_arrow = input.parse()?;
		let handler = input.parse()?;
		let comma = input.parse().ok();

		Ok(RouteEntry {
			method,
			path,
			middlewares,
			_fat_arrow: fat_arrow,
			handler,
			_comma: comma,
		})
	}
}

impl Parse for RouteEntry {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let method: Ident = input.parse()?;
		Self::parse_with_method(input, method)
	}
}

struct FinalRoute {
	method: Ident,
	path: String,
	handler: Path,
	middleware_stack: Vec<Path>,
}

fn flatten_items(
	items: &[RouterItem],
	current_prefix: &str,
	current_middleware: &[Path],
	flat_routes: &mut Vec<FinalRoute>,
) {
	let mut local_middleware = current_middleware.to_vec();

	for item in items {
		match item {
			RouterItem::Middleware(m) => {
				local_middleware.push(m.middleware.clone());
			}
			RouterItem::Route(r) => {
				let mut route_middleware = local_middleware.clone();
				route_middleware.extend(r.middlewares.clone());

				let full_path = format!("{}{}", current_prefix, r.path.value());
				flat_routes.push(FinalRoute {
					method: r.method.clone(),
					path: full_path,
					handler: r.handler.clone(),
					middleware_stack: route_middleware,
				});
			}
			RouterItem::Group(g) => {
				let new_prefix = format!("{}{}", current_prefix, g.prefix.value());
				flatten_items(&g.items, &new_prefix, &local_middleware, flat_routes);
			}
		}
	}
}

/// Implementation logic for the `router!` macro.
///
/// This function parses the DSL-like syntax of the `router!` macro and generates:
/// - A struct representing the router (with optional state).
/// - An implementation of the `Server` trait for that router.
/// - Efficient routing logic that dispatches requests to the appropriate `RouteHandler`.
///
/// The routing logic supports:
/// - Static paths (e.g., "/users")
/// - Named parameters (e.g., "/users/:id")
/// - Rest parameters (e.g., "/static/*path")
/// - Method matching (GET, POST, etc.)
///
/// # Syntax Example
///
/// ```ignore
/// router! {
///     MyRouter<MyState> {
///         get("/users") => get_users,
///         post("/users/:id") => create_user,
///         get("/static/*path") => serve_static
///     }
/// }
/// ```
pub fn router_impl(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let input = parse_macro_input!(item as RouterInput);
	let visibility = input.visibility;
	let router_name = input.name;

	let mut flat_routes = Vec::new();
	flatten_items(&input.items, "", &[], &mut flat_routes);

	let route_logic = generate_route_logic(&flat_routes, input.state_type.is_some());

	let (state, new) = if let Some(state_ty) = input.state_type {
		(
			quote! { (pub #state_ty) },
			quote! {
			   pub fn new(state: #state_ty) -> Self {
				   Self(state)
			   }
			},
		)
	} else {
		(
			TokenStream::new(),
			quote! {
				pub fn new() -> Self {
					Self
				}
			},
		)
	};

	let output = quote! {
			#visibility struct #router_name #state;

			impl #router_name {
				#new
			}

			impl ::moonbeam::Server for #router_name {
				fn route(&'static self, req: ::moonbeam::http::Request<'_, '_>) -> impl ::std::future::Future<Output = ::moonbeam::http::Response> {
					async move {
						let method = req.method;
						let path = req.url();
						let mut path_segments = [""; 8];
						let len: usize = path.split('/').filter(|s| !s.is_empty()).zip(&mut path_segments).fold(0, |count, (src, dst)| {
							*dst = src;
							count + 1
						});

						#route_logic

						::moonbeam::http::Response::not_found()
					}
				}
			}
	};

	output.into()
}

fn generate_route_logic(routes: &[FinalRoute], has_state: bool) -> TokenStream {
	let mut routes_by_method: HashMap<String, Vec<&FinalRoute>> = HashMap::new();

	for route in routes {
		let method = route.method.to_string().to_uppercase();
		routes_by_method.entry(method).or_default().push(route);
	}

	let mut method_match_arms = TokenStream::new();
	let state = if has_state {
		quote! { &self.0 }
	} else {
		quote! { self }
	};

	for (method, mut method_routes) in routes_by_method {
		// Sort routes: Literal < Param < Rest
		method_routes.sort_by(|a, b| {
			let a_path = &a.path;
			let b_path = &b.path;
			let mut a_segments = a_path.split('/').filter(|s| !s.is_empty());
			let mut b_segments = b_path.split('/').filter(|s| !s.is_empty());

			// Iterate segments
			for (seg_a, seg_b) in (&mut a_segments).zip(&mut b_segments) {
				let type_a = if seg_a.starts_with(':') {
					1
				} else if seg_a.starts_with('*') {
					2
				} else {
					0
				};
				let type_b = if seg_b.starts_with(':') {
					1
				} else if seg_b.starts_with('*') {
					2
				} else {
					0
				};

				if type_a != type_b {
					return type_a.cmp(&type_b);
				}
			}

			// If prefix matches, sort longer one first
			if a_segments.next().is_some() {
				std::cmp::Ordering::Less
			} else {
				std::cmp::Ordering::Greater
			}
		});

		let mut path_match_arms = TokenStream::new();

		for route in method_routes {
			let path_str = &route.path;
			let handler = &route.handler;
			let segments = path_str.split('/').filter(|s| !s.is_empty());

			let mut pattern_tokens = Vec::new();
			let mut params_items = Vec::new();

			for (i, segment) in segments.enumerate() {
				if segment.starts_with(':') {
					// Named parameter
					let bind_name = Ident::new(&format!("p{}", i), proc_macro2::Span::call_site());
					pattern_tokens.push(quote! { #bind_name });
					params_items.push(quote! { *#bind_name });
				} else if segment.starts_with('*') {
					// Rest parameter
					let bind_name = Ident::new(&format!("p{}", i), proc_macro2::Span::call_site());
					pattern_tokens.push(quote! { #bind_name @ .. });

					params_items.push(quote! {
						if #bind_name.is_empty() {
							""
						} else {
							let start = #bind_name[0].as_ptr() as usize - path.as_ptr() as usize;
							&path[start..]
						}
					});
				} else {
					// Literal
					pattern_tokens.push(quote! { #segment });
				}
			}

			let pattern = quote! { [ #(#pattern_tokens),* ] };
			let params_block = quote! {
				let params = [ #(#params_items),* ];
			};

			// Build the middleware chain
			// Start with the handler wrapped in a future that converts output to Response
			let mut call_chain = quote! {
				async move {
					::moonbeam::router::RouteHandler::call(&#handler, req, &params, #state).await.into()
				}
			};

			// Wrap with middlewares
			for middleware in route.middleware_stack.iter().rev() {
				call_chain = quote! {
					#middleware(req, #state, |req| #call_chain)
				};
			}

			path_match_arms.extend(quote! {
				#pattern => {
					#params_block
					return #call_chain.await.into();
				}
			});
		}

		// Add default arm for path match
		path_match_arms.extend(quote! {
			_ => {}
		});

		method_match_arms.extend(quote! {
			if method.eq_ignore_ascii_case(#method) {
				match &path_segments[..len] {
					#path_match_arms
				}
			}
		});
	}

	method_match_arms
}
