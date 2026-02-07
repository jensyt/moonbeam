use proc_macro2::TokenStream;
use quote::quote;
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
	CatchAll(CatchAllEntry),
}

struct CatchAllEntry {
	_underscore: Token![_],
	_fat_arrow: Token![=>],
	handler: Handler,
}

#[derive(Clone)]
enum Handler {
	Path(Path),
	Bang(Token![!]),
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

impl Parse for Handler {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		if input.peek(Token![!]) {
			Ok(Handler::Bang(input.parse()?))
		} else {
			Ok(Handler::Path(input.parse()?))
		}
	}
}

impl Parse for RouterItem {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		if input.peek(LitStr) {
			Ok(RouterItem::Group(input.parse()?))
		} else if input.peek(Token![_]) {
			Ok(RouterItem::CatchAll(CatchAllEntry {
				_underscore: input.parse()?,
				_fat_arrow: input.parse()?,
				handler: input.parse()?,
			}))
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
			Err(input.error("expected route group, route, middleware, or catch-all"))
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

#[derive(Clone)]
struct FinalRoute {
	method: String,
	path: String,
	handler: Handler,
	middleware_stack: Vec<Path>,
	is_fallback: bool,
}

fn flatten_items(
	items: &[RouterItem],
	current_prefix: &str,
	current_middleware: &[Path],
	flat_routes: &mut Vec<FinalRoute>,
) {
	let mut local_middleware = current_middleware.to_vec();
	let mut has_catchall = false;

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
					method: r.method.to_string().to_uppercase(),
					path: full_path,
					handler: Handler::Path(r.handler.clone()),
					middleware_stack: route_middleware,
					is_fallback: false,
				});
			}
			RouterItem::CatchAll(c) => {
				has_catchall = true;
				let route_middleware = local_middleware.clone();
				// CatchAll applies to the current prefix
				// We represent the path as the prefix itself
				// If prefix is empty (root), path is empty string
				let full_path = current_prefix.to_string();
				flat_routes.push(FinalRoute {
					method: "ANY".to_string(), // Special method
					path: full_path,
					handler: c.handler.match_cloned(),
					middleware_stack: route_middleware,
					is_fallback: true,
				});
			}
			RouterItem::Group(g) => {
				let new_prefix = format!("{}{}", current_prefix, g.prefix.value());
				flatten_items(&g.items, &new_prefix, &local_middleware, flat_routes);
			}
		}
	}

	// Insert default catchall at root level if needed
	if !has_catchall && current_prefix.is_empty() {
		let route_middleware = local_middleware.clone();
		let full_path = current_prefix.to_string();
		flat_routes.push(FinalRoute {
			method: "ANY".to_string(),
			path: full_path,
			handler: Handler::Bang(Default::default()),
			middleware_stack: route_middleware,
			is_fallback: true,
		});
	}
}

impl Handler {
	fn match_cloned(&self) -> Self {
		match self {
			Handler::Path(p) => Handler::Path(p.clone()),
			Handler::Bang(b) => Handler::Bang(*b),
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
				fn route(&'static self, req: ::moonbeam::http::Request<'_, '_>) ->
					impl ::std::future::Future<Output = ::moonbeam::http::Response>
				{
					async move {
						let method = req.method;
						let path = req.url();
						let mut path_segments = [""; 8];
						let len: usize = path
							.split('/')
							.filter(|s| !s.is_empty())
							.zip(&mut path_segments)
							.fold(0, |count, (src, dst)| {
								*dst = src;
								count + 1
							});

						#route_logic
					}
				}
			}
	};

	output.into()
}

fn generate_route_logic(routes: &[FinalRoute], has_state: bool) -> TokenStream {
	#[allow(unused_mut)]
	let mut all_routes = routes.to_vec();

	#[cfg(feature = "autohead")]
	{
		let mut head_routes = Vec::new();
		for route in all_routes
			.iter()
			.filter(|r| r.method == "GET" && !r.is_fallback)
		{
			if !all_routes
				.iter()
				.any(|r| r.method == "HEAD" && r.path == route.path)
			{
				let mut head_route = route.clone();
				head_route.method = "HEAD".to_string();
				head_routes.push(head_route);
			}
		}
		all_routes.extend(head_routes);
	}

	let mut all_routes_refs: Vec<&FinalRoute> = all_routes.iter().collect();
	sort_routes(&mut all_routes_refs);

	let state = if has_state {
		quote! { &self.0 }
	} else {
		quote! { self }
	};

	let path_match_arms = generate_path_arms(&all_routes_refs, &state);

	quote! {
		match &path_segments[..len] {
			#path_match_arms
		}
	}
}

fn sort_routes(routes: &mut Vec<&FinalRoute>) {
	routes.sort_by(|a, b| {
		// Fallbacks always come after specific routes
		let a_path = &a.path;
		let b_path = &b.path;
		let mut a_segments = a_path.split('/').filter(|s| !s.is_empty());
		let mut b_segments = b_path.split('/').filter(|s| !s.is_empty());

		// Iterate segments
		loop {
			match (a_segments.next(), b_segments.next()) {
				(Some(sa), Some(sb)) => {
					let type_a = if sa.starts_with(':') {
						1
					} else if sa.starts_with('*') {
						2
					} else {
						0
					};
					let type_b = if sb.starts_with(':') {
						1
					} else if sb.starts_with('*') {
						2
					} else {
						0
					};

					if type_a != type_b {
						return type_a.cmp(&type_b);
					}
					// If both are literals, sort alphabetically
					if type_a == 0 {
						let cmp = sa.cmp(sb);
						if cmp != std::cmp::Ordering::Equal {
							return cmp;
						}
					}
				}
				(Some(_), None) => {
					return std::cmp::Ordering::Less;
				}
				(None, Some(_)) => {
					return std::cmp::Ordering::Greater;
				}
				(None, None) => {
					if a.is_fallback && !b.is_fallback {
						return std::cmp::Ordering::Greater;
					} else if !a.is_fallback && b.is_fallback {
						return std::cmp::Ordering::Less;
					}
					// Stable sort for same pattern to ensure consistent grouping
					return a.method.cmp(&b.method);
				}
			}
		}
	});
}

fn make_rest_param(
	bind_name: Ident,
	pattern_tokens: &mut Vec<TokenStream>,
	params_items: &mut Vec<TokenStream>,
) {
	pattern_tokens.push(quote! { #bind_name @ .. });

	params_items.push(quote! {
		if #bind_name.is_empty() {
			""
		} else {
			let start = #bind_name[0].as_ptr() as usize - path.as_ptr() as usize;
			&path[start..]
		}
	});
}

fn generate_path_arms(routes: &[&FinalRoute], state: &TokenStream) -> TokenStream {
	let mut path_match_arms = TokenStream::new();

	let mut i = 0;
	while i < routes.len() {
		let route = routes[i];
		let mut group = vec![route];

		let mut j = i + 1;
		while j < routes.len() {
			if is_same_pattern(route, routes[j]) {
				group.push(routes[j]);
				j += 1;
			} else {
				break;
			}
		}

		path_match_arms.extend(generate_group_arm(&group, state));
		i = j;
	}

	path_match_arms
}

fn is_same_pattern(a: &FinalRoute, b: &FinalRoute) -> bool {
	if a.path != b.path || a.is_fallback != b.is_fallback {
		return false;
	}
	if a.is_fallback {
		matches!(
			(&a.handler, &b.handler),
			(Handler::Path(_), Handler::Path(_)) | (Handler::Bang(_), Handler::Bang(_))
		)
	} else {
		true
	}
}

fn generate_group_arm(group: &[&FinalRoute], state: &TokenStream) -> TokenStream {
	let first = group[0];
	let path_str = &first.path;
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
			make_rest_param(bind_name, &mut pattern_tokens, &mut params_items);
		} else {
			// Literal
			pattern_tokens.push(quote! { #segment });
		}
	}

	if first.is_fallback {
		match &first.handler {
			Handler::Path(_) => {
				let bind_name = Ident::new("rest", proc_macro2::Span::call_site());
				make_rest_param(bind_name, &mut pattern_tokens, &mut params_items);
			}
			Handler::Bang(_) => {
				pattern_tokens.push(quote! { .. });
			}
		}
	}

	let pattern = quote! { [ #(#pattern_tokens),* ] };

	let params_block = if first.is_fallback && matches!(first.handler, Handler::Bang(_)) {
		quote! {}
	} else {
		quote! { let params = [ #(#params_items),* ]; }
	};

	let mut method_checks = TokenStream::new();
	let mut allowed_methods = Vec::new();
	let mut has_any = false;

	for route in group {
		let handler = &route.handler;

		// Build the middleware chain
		let mut call_chain = match handler {
			Handler::Path(p) => {
				quote! {
					::moonbeam::router::RouteHandler::call(&#p, req, &params, #state).await.into()
				}
			}
			Handler::Bang(_) => quote! {
				::moonbeam::http::Response::not_found()
			},
		};

		if !route.middleware_stack.is_empty() {
			call_chain = quote! {
				async move {
					#call_chain
				}
			};

			// Wrap with middlewares
			for middleware in route.middleware_stack.iter().rev() {
				call_chain = quote! {
					#middleware(req, #state, |req| #call_chain)
				};
			}

			call_chain = quote! {
				#call_chain.await
			}
		}

		if route.method == "ANY" {
			has_any = true;
			method_checks.extend(quote! {
				{
					#call_chain
				}
			});
			// If ANY is present, it catches everything, so we stop checking.
			break;
		} else {
			allowed_methods.push(route.method.as_str());
			let method_name = &route.method;
			method_checks.extend(quote! {
				if method.eq_ignore_ascii_case(#method_name) {
					#call_chain
				} else
			});
		}
	}

	if !has_any {
		let allowed = allowed_methods.join(", ");
		method_checks.extend(quote! {
			{
				::moonbeam::http::Response::method_not_allowed()
					.with_header("Allow", #allowed)
			}
		});
	}

	quote! {
		#pattern => {
			#params_block
			#method_checks
		}
	}
}
