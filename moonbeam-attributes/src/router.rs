use proc_macro2::TokenStream;
use quote::quote;
use syn::{
	Ident, LitStr, Token,
	parse::{Parse, ParseStream},
	parse_macro_input,
};

struct RouterInput {
	name: Ident,
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

		let content;
		syn::braced!(content in input);

		let mut routes = Vec::new();
		while !content.is_empty() {
			routes.push(content.parse()?);
		}

		Ok(RouterInput { name, routes })
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

pub fn router_impl(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let input = parse_macro_input!(item as RouterInput);
	let router_name = input.name;

	let route_logic = generate_route_logic(&input.routes);

	let output = quote! {
		struct #router_name<S>(pub S);

		impl<S> #router_name<S> {
			pub fn new(state: S) -> Self {
				Self(state)
			}
		}

		impl #router_name<()> {
			 pub fn stateless() -> Self {
				 Self(())
			 }
		}

		// Send + Sync removed here as requested
		impl<S: 'static> ::moonbeam::Server for #router_name<S> {
			fn route(&'static self, req: ::moonbeam::http::Request<'_, '_>) -> impl ::std::future::Future<Output = ::moonbeam::http::Response> {
				async move {
					let method = req.method;
					let path = req.path;

					#route_logic

					::moonbeam::http::Response::not_found()
				}
			}
		}
	};

	output.into()
}

fn generate_route_logic(routes: &[RouteEntry]) -> TokenStream {
	let mut checks = TokenStream::new();

	for route in routes {
		let method_str = route.method.to_string().to_uppercase();
		let path_str = route.path.value();
		let handler = &route.handler;

		let segments: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
		let segment_count = segments.len();

		let mut param_extraction = TokenStream::new();
		let mut path_checks = TokenStream::new();

		for (i, segment) in segments.iter().enumerate() {
			if segment.starts_with(':') {
				let param_name = &segment[1..];
				param_extraction.extend(quote! {
					params.insert(#param_name.to_string(), path_segments[#i].to_string());
				});
			} else {
				let seg_lit = segment.to_string();
				path_checks.extend(quote! {
					if path_segments[#i] != #seg_lit { mismatch = true; }
				});
			}
		}

		checks.extend(quote! {
			if method.eq_ignore_ascii_case(#method_str) {
				let path_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
				if path_segments.len() == #segment_count {
					let mut mismatch = false;
					#path_checks

					if !mismatch {
						let mut params = ::std::collections::HashMap::new();
						#param_extraction

						return ::moonbeam::router::RouteHandler::call(&#handler, req, params, &self.0).await;
					}
				}
			}
		});
	}

	checks
}
