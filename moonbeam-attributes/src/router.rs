use proc_macro2::TokenStream;
use quote::quote;
use std::cmp::Ordering;
use std::collections::HashMap;
use syn::{
	Ident, LitStr, Token, Type,
	parse::{Parse, ParseStream},
	parse_macro_input,
};

struct RouterInput {
	name: Ident,
	state_type: Option<Type>,
	routes: Vec<RouteEntry>,
}

struct RouteEntry {
	method: Ident,
	path: LitStr,
	_fat_arrow: Token![=>],
	handler: Ident,
	_comma: Option<Token![,]>,
}

impl Parse for RouterInput {
	fn parse(input: ParseStream) -> syn::Result<Self> {
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

		let mut routes = Vec::new();
		while !content.is_empty() {
			routes.push(content.parse()?);
		}

		Ok(RouterInput {
			name,
			state_type,
			routes,
		})
	}
}

impl Parse for RouteEntry {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let method: Ident = input.parse()?;

		let content;
		syn::parenthesized!(content in input);
		let path: LitStr = content.parse()?;

		let fat_arrow = input.parse()?;
		let handler = input.parse()?;
		let comma = input.parse().ok();

		Ok(RouteEntry {
			method,
			path,
			_fat_arrow: fat_arrow,
			handler,
			_comma: comma,
		})
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
/// - Method matching (GET, POST, etc.)
///
/// # Syntax Example
///
/// ```ignore
/// router! {
///     MyRouter<MyState> {
///         get("/users") => get_users,
///         post("/users/:id") => create_user
///     }
/// }
/// ```
pub fn router_impl(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let input = parse_macro_input!(item as RouterInput);
	let router_name = input.name;

	let route_logic = generate_route_logic(&input.routes, input.state_type.is_some());

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
			struct #router_name #state;

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

fn generate_route_logic(routes: &[RouteEntry], has_state: bool) -> TokenStream {
	let mut routes_by_method: HashMap<String, Vec<&RouteEntry>> = HashMap::new();

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
		// Sort routes to ensure literals match before params
		method_routes.sort_by(|a, b| {
			let a_path = a.path.value();
			let b_path = b.path.value();
			let a_segments: Vec<&str> = a_path.split('/').filter(|s| !s.is_empty()).collect();
			let b_segments: Vec<&str> = b_path.split('/').filter(|s| !s.is_empty()).collect();

			// Iterate segments
			for (seg_a, seg_b) in a_segments.iter().zip(b_segments.iter()) {
				let a_is_param = seg_a.starts_with(':');
				let b_is_param = seg_b.starts_with(':');

				if !a_is_param && b_is_param {
					return Ordering::Less;
				}
				if a_is_param && !b_is_param {
					return Ordering::Greater;
				}
			}

			// If prefix matches, longer path (more specific) should be checked first?
			// Actually slice matching handles length, so disjoint lengths are fine.
			// But for consistency:
			a_segments.len().cmp(&b_segments.len())
		});

		let mut path_match_arms = TokenStream::new();

		for route in method_routes {
			let path_str = route.path.value();
			let handler = &route.handler;
			let segments: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();

			let mut pattern_tokens = Vec::new();
			let mut params_items = Vec::new();

			for (i, segment) in segments.iter().enumerate() {
				if segment.starts_with(':') {
					// Use a unique name for the match binding to avoid collisions
					let bind_name = Ident::new(&format!("p{}", i), proc_macro2::Span::call_site());
					pattern_tokens.push(quote! { #bind_name });

					params_items.push(quote! {
						*#bind_name
					});
				} else {
					pattern_tokens.push(quote! { #segment });
				}
			}

			let pattern = quote! { [ #(#pattern_tokens),* ] };

			path_match_arms.extend(quote! {
				#pattern => {
					let params = [ #(#params_items),* ];
					return ::moonbeam::router::RouteHandler::call(&#handler, req, &params, #state).await;
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
