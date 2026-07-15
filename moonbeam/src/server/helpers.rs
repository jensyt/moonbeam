//! Helper types for implementing [`Server`] without macros.

use super::Server;
use super::task::Spawner;
use crate::http::{Request, Response};
use std::marker::PhantomData;

/// Helper struct to construct stateless servers from an async function.
///
/// To implement [`Server`], `H` must have the following signature:
/// ```ignore
/// async fn(Request, Spawner) -> Response
/// ```
///
/// # Example
/// ```
/// use moonbeam::{Request, Response, Spawner};
/// use moonbeam::server::LifetimeDummy;
///
/// async fn handle<'exec, 'req>(
///     request: Request<'req, 'req>,
///     _spawner: Spawner<'exec>,
///     _: LifetimeDummy<'exec, 'req>
/// ) -> Response<'req> {
///     // Note the `move` here: while we can use the request in the response, we need to move it
///     // into the closure so it outlives this function.
///     Response::new_from_sse_fn(async move |writer| {
///         writer.write(&request.path);
///     })
/// }
/// ```
///
/// Note that [`LifetimeDummy`] is a zero-sized type that is used to bind the lifetime of the
/// request and spawner together, since Rust higher-ranked trait bounds do not support requiring
/// `'exec: 'req` on the async fn.
pub struct StatelessAsyncFnServer<H>(H);
impl<H> StatelessAsyncFnServer<H> {
	/// Construct a new [`StatelessAsyncFnServer`] from the given async function.
	pub fn new(h: H) -> Self {
		Self(h)
	}
}

impl<H> Server for StatelessAsyncFnServer<H>
where
	H: for<'exec, 'req> AsyncFn(
		Request<'req, 'req>,
		Spawner<'exec>,
		LifetimeDummy<'exec, 'req>,
	) -> Response<'req>,
{
	fn route<'exec: 'req, 'req>(
		&'exec self,
		request: Request<'req, 'req>,
		spawner: Spawner<'exec>,
	) -> impl Future<Output = Response<'req>> {
		self.0(request, spawner, LifetimeDummy::default())
	}
}

/// Helper struct to construct stateful servers from an async function.
///
/// To implement [`Server`], `H` must have the following signature:
/// ```ignore
/// async fn(Request, Spawner, &State) -> Response
/// ```
///
/// # Example
/// ```
/// use moonbeam::{Request, Response, Spawner};
/// use moonbeam::server::LifetimeDummy;
///
/// struct State(&'static str);
///
/// async fn handle<'exec, 'req>(
///     request: Request<'req, 'req>,
///     _spawner: Spawner<'exec>,
///     state: &'exec State,
///     _: LifetimeDummy<'exec, 'req>,
/// ) -> Response<'req> {
///     // Note the `move` here: while we can use the request in the response, we need to move it
///     // into the closure so it outlives this function.
///     Response::new_from_sse_fn(async move |writer| {
///         writer.write_string(format!("{} - {}", request.path, state.0));
///     })
/// }
/// ```
///
/// Note that [`LifetimeDummy`] is a zero-sized type that is used to bind the lifetime of the
/// request and state together, since Rust higher-ranked trait bounds do not support requiring
/// `'exec: 'req` on the async fn.
pub struct AsyncFnServer<H, S>(H, S);
impl<H, S> AsyncFnServer<H, S> {
	/// Construct a new [`AsyncFnServer`] from the given async function and initial state.
	pub fn new(h: H, state: S) -> Self {
		Self(h, state)
	}
}

impl<H, S> Server for AsyncFnServer<H, S>
where
	H: for<'exec, 'req> AsyncFn(
		Request<'req, 'req>,
		Spawner<'exec>,
		&'exec S,
		LifetimeDummy<'exec, 'req>,
	) -> Response<'req>,
{
	fn route<'exec: 'req, 'req>(
		&'exec self,
		request: Request<'req, 'req>,
		spawner: Spawner<'exec>,
	) -> impl Future<Output = Response<'req>> {
		self.0(request, spawner, &self.1, LifetimeDummy::default())
	}
}

/// A dummy lifetime bound that allows the response to reference the request and executor.
pub struct LifetimeDummy<'exec, 'req>(PhantomData<&'req &'exec ()>);

impl<'exec, 'req> Default for LifetimeDummy<'exec, 'req> {
	fn default() -> Self {
		LifetimeDummy(PhantomData)
	}
}
