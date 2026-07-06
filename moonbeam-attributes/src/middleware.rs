use quote::quote;
use syn::{FnArg, ItemFn, PathArguments, Type, parse_quote, spanned::Spanned};

pub(super) fn middleware_impl(
	_attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	match do_middleware_impl(item) {
		Ok(tokens) => tokens,
		Err(err) => err.to_compile_error().into(),
	}
}

fn do_middleware_impl(
	item: proc_macro::TokenStream,
) -> Result<proc_macro::TokenStream, syn::Error> {
	let mut input_fn: ItemFn = syn::parse(item)?;

	let inputs = &mut input_fn.sig.inputs;
	let inputs_span = inputs.span();
	if inputs.len() != 4 {
		return Err(syn::Error::new(
			inputs_span,
			"Middleware must have exactly 4 arguments: (Request, Spawner, State, Next)",
		));
	}

	// Identify the state type parameter to distinguish it from the future return type
	let mut state_type_param_ident = None;
	let arg3_ref = &inputs[2];
	if let FnArg::Typed(pat_type) = arg3_ref
		&& let Type::Reference(type_ref) = &*pat_type.ty
		&& let Type::Path(type_path) = &*type_ref.elem
		&& let Some(ident) = type_path.path.get_ident()
	{
		for param in &input_fn.sig.generics.params {
			if let syn::GenericParam::Type(ty) = param
				&& ty.ident == *ident
			{
				state_type_param_ident = Some(ident.clone());
				break;
			}
		}
	}

	// Identify or default the future return type identifier (non-state type parameter)
	let mut fut_ty_ident = None;
	for param in &input_fn.sig.generics.params {
		if let syn::GenericParam::Type(ty) = param
			&& Some(&ty.ident) != state_type_param_ident.as_ref()
		{
			fut_ty_ident = Some(ty.ident.clone());
			break;
		}
	}
	let fut_ty_ident = fut_ty_ident.unwrap_or_else(|| quote::format_ident!("Fut"));

	let mut req_lt_a: Option<syn::Lifetime> = None;
	let mut req_lt_b: Option<syn::Lifetime> = None;
	let mut spawner_lt: Option<syn::Lifetime> = None;

	let mut inputs_iter = inputs.iter_mut();

	// Arg 1: Request
	let arg1 = inputs_iter
		.next()
		.ok_or_else(|| syn::Error::new(inputs_span, "Missing Request argument"))?;
	let seg1 = extract_path_segment(arg1, "First", "Request")?;
	let mut has_req_lifetimes = false;
	if let PathArguments::AngleBracketed(ab) = &seg1.arguments {
		let mut lts = ab.args.iter().filter_map(|arg| {
			if let syn::GenericArgument::Lifetime(lt) = arg {
				if lt.ident != "_" {
					Some(lt.clone())
				} else {
					None
				}
			} else {
				None
			}
		});
		if let Some(la) = lts.next()
			&& let Some(lb) = lts.next()
		{
			req_lt_a = Some(la);
			req_lt_b = Some(lb);
			has_req_lifetimes = true;
		}
	}
	if !has_req_lifetimes {
		seg1.arguments = PathArguments::AngleBracketed(parse_quote!(<'req, 'req>));
		req_lt_a = Some(parse_quote!('req));
		req_lt_b = Some(parse_quote!('req));
	}

	// Arg 2: Spawner
	let arg2 = inputs_iter
		.next()
		.ok_or_else(|| syn::Error::new(inputs_span, "Missing Spawner argument"))?;
	let seg2 = extract_path_segment(arg2, "Second", "Spawner")?;
	let mut has_spawner_lifetime = false;
	if let PathArguments::AngleBracketed(ab) = &seg2.arguments {
		for g_arg in &ab.args {
			if let syn::GenericArgument::Lifetime(lt) = g_arg {
				if lt.ident != "_" {
					spawner_lt = Some(lt.clone());
					has_spawner_lifetime = true;
				}
				break;
			}
		}
	}
	if !has_spawner_lifetime {
		seg2.arguments = PathArguments::AngleBracketed(parse_quote!(<'exec>));
		spawner_lt = Some(parse_quote!('exec));
	}

	// Arg 3: State
	let arg3 = inputs_iter
		.next()
		.ok_or_else(|| syn::Error::new(inputs_span, "Missing State argument"))?;
	match arg3 {
		FnArg::Typed(pat_type) => {
			if let Type::Reference(t) = &mut *pat_type.ty {
				if t.lifetime.as_ref().is_none_or(|v| v.ident == "_") {
					t.lifetime = Some(parse_quote!('exec));
				}
			} else {
				return Err(syn::Error::new(
					pat_type.ty.span(),
					"Third argument State must be a reference",
				));
			}
		}
		FnArg::Receiver(recv) => {
			return Err(syn::Error::new(
				recv.span(),
				"Third argument must be State, not self",
			));
		}
	}

	// Arg 4: Next
	let arg4 = inputs_iter
		.next()
		.ok_or_else(|| syn::Error::new(inputs_span, "Missing Next argument"))?;
	extract_path_segment(arg4, "Fourth", "Next")?;
	let la = req_lt_a.as_ref().unwrap();
	let lb = req_lt_b.as_ref().unwrap();
	if let FnArg::Typed(pat_type) = arg4 {
		pat_type.ty = parse_quote!(impl FnOnce(::moonbeam::Request<#la, #lb>) -> #fut_ty_ident);
	}

	// 3. Inject and merge generic parameters cleanly
	let mut lifetimes = Vec::new();
	let mut types = Vec::new();
	let mut consts = Vec::new();

	let params = std::mem::take(&mut input_fn.sig.generics.params);
	for param in params {
		match param {
			syn::GenericParam::Lifetime(lt) => lifetimes.push(lt),
			syn::GenericParam::Type(ty) => types.push(ty),
			syn::GenericParam::Const(c) => consts.push(c),
		}
	}

	if let Some(ref lt) = req_lt_a
		&& !lifetimes.iter().any(|l| l.lifetime.ident == lt.ident)
	{
		let lt_param: syn::LifetimeParam = parse_quote!(#lt);
		lifetimes.push(lt_param);
	}
	if let Some(ref lt) = req_lt_b
		&& !lifetimes.iter().any(|l| l.lifetime.ident == lt.ident)
	{
		let lt_param: syn::LifetimeParam = parse_quote!(#lt);
		lifetimes.push(lt_param);
	}
	if let Some(ref lt) = spawner_lt
		&& !lifetimes.iter().any(|l| l.lifetime.ident == lt.ident)
	{
		let lt_param: syn::LifetimeParam = parse_quote!(#lt);
		lifetimes.push(lt_param);
	}

	if !types.iter().any(|t| t.ident == fut_ty_ident) {
		let ty_param: syn::TypeParam = parse_quote!(#fut_ty_ident);
		types.push(ty_param);
	}

	for lt in lifetimes {
		input_fn
			.sig
			.generics
			.params
			.push(syn::GenericParam::Lifetime(lt));
	}
	for ty in types {
		input_fn
			.sig
			.generics
			.params
			.push(syn::GenericParam::Type(ty));
	}
	for c in consts {
		input_fn
			.sig
			.generics
			.params
			.push(syn::GenericParam::Const(c));
	}

	// 4. Add where clause: where Fut: Future<Output = Response>
	let where_clause = input_fn.sig.generics.make_where_clause();
	let mut bound_exists = false;
	for pred in &where_clause.predicates {
		if let syn::WherePredicate::Type(pred_type) = pred
			&& let Type::Path(type_path) = &pred_type.bounded_ty
			&& type_path.path.is_ident(&fut_ty_ident)
		{
			bound_exists = true;
			break;
		}
	}

	if !bound_exists {
		where_clause.predicates.push(
			parse_quote!(#fut_ty_ident: ::std::future::Future<Output = ::moonbeam::Response<#la>>),
		);
	}

	if super::check_response(&mut input_fn.sig.output, la).is_none() {
		let span = input_fn.sig.output.span();
		return Err(syn::Error::new(span, "Output must be: Response"));
	}

	Ok(quote! {
		#input_fn
	}
	.into())
}

/// Helper function to extract a mutable reference to the last segment of a type path
/// and validate that the type matches the expected identifier name.
fn extract_path_segment<'a>(
	arg: &'a mut FnArg,
	arg_name: &str,
	expected: &str,
) -> Result<&'a mut syn::PathSegment, syn::Error> {
	let typed = match arg {
		FnArg::Typed(t) => t,
		FnArg::Receiver(r) => {
			return Err(syn::Error::new(
				r.span(),
				format!("{} argument must be {}, not self", arg_name, expected),
			));
		}
	};
	let path_type = match &mut *typed.ty {
		Type::Path(p) => p,
		other => {
			return Err(syn::Error::new(
				other.span(),
				format!("{} argument must be {}", arg_name, expected),
			));
		}
	};
	let type_span = path_type.span();
	let last = path_type.path.segments.last_mut().ok_or_else(|| {
		syn::Error::new(
			type_span,
			format!("{} argument must be {}", arg_name, expected),
		)
	})?;
	if last.ident != expected {
		return Err(syn::Error::new(
			last.span(),
			format!("{} argument must be {}", arg_name, expected),
		));
	}
	Ok(last)
}
