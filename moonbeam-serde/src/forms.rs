//! Support for deserializing form data from HTTP requests.
//!
//! This module provides a [`Form`] extractor that can be used in route handlers to extract form
//! data from HTTP requests. It supports both URL-encoded and multipart form data.
//!
//! For URL-encoded data, make sure to use `Cow<'a, str>` for string fields with the
//! `#[serde(borrow)]` attribute to support zero-copy borrowing. This is required because URL
//! decoding may create a new string allocation.
//!
//! For multipart data, you can always use `&str` since it never allocators. Multipart also supports
//! files through the special [`File`] type.
//!
//! # Examples
//! ```rust,no_run
//! use moonbeam::route;
//! use moonbeam::http::{Body, Response};
//! use moonbeam_serde::{Form, File};
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct Upload<'a> {
//!     title: &'a str,
//!     file: File<'a>,
//! }
//!
//! #[route]
//! async fn handle_upload(Form(u): Form<Upload<'_>>) -> Response {
//!     Response::ok().with_body(
//!         format!("{}:{}:{}", u.title, u.file.name.unwrap_or(""), u.file.data.len()),
//!         Body::TEXT,
//!     )
//! }
//! ```

use moonbeam::http::{FromRequest, Request, Response, params::AllParamIter};
use moonbeam_forms::{Form as RawForm, FormData};
use paste::paste;
use serde::de::{self, IntoDeserializer, Visitor, value::BorrowedBytesDeserializer};
use serde::{Deserialize, forward_to_deserialize_any};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// A wrapper for form-data request bodies.
///
/// This struct implements `FromRequest`, allowing it to be used as an extractor
/// in route handlers. It supports both URL-encoded and multipart form data.
///
/// # Example
///
/// ```rust,no_run
/// use moonbeam::route;
/// use moonbeam::http::{Body, Response};
/// use moonbeam_serde::Form;
/// use serde::Deserialize;
/// use std::borrow::Cow;
///
/// #[derive(Deserialize)]
/// struct User<'a> {
///     #[serde(borrow)]
///     name: Cow<'a, str>,
///     age: u32,
/// }
///
/// #[route]
/// async fn hello_user(Form(user): Form<User<'_>>) -> Response {
///     Response::ok().with_body(
///         format!(
///             "Hello, {} (age: {})!",
///              user.name, user.age
///         ),
///         Body::TEXT,
///     )
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Form<T>(pub T);

/// A wrapper for file uploads in form data.
///
/// This can be used in a struct with `Form` to extract file information.
///
/// # Example
///
/// ```rust,no_run
/// use moonbeam_serde::File;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Upload<'a> {
///     title: &'a str,
///     #[serde(borrow)]
///     file: File<'a>,
/// }
/// ```
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct File<'a> {
	/// The original filename provided by the client, if any.
	pub name: Option<&'a str>,
	/// The content type of the file, if any.
	pub content_type: Option<&'a str>,
	/// The raw bytes of the file.
	pub data: &'a [u8],
}

/// An error that can occur when deserializing a form.
#[derive(Debug)]
pub enum SerdeFormError {
	/// An error that occurred while parsing the form data.
	FormError(moonbeam_forms::FormError),
	/// An error that occurred while deserializing the form data.
	SerdeError(String),
	/// Unsupported form type.
	UnsupportedFormType,
}

impl From<SerdeFormError> for Response {
	fn from(value: SerdeFormError) -> Self {
		match value {
			SerdeFormError::FormError(e) => e.into(),
			SerdeFormError::SerdeError(e) => Response::bad_request().with_body(
				format!("Form deserialization failed: {}", e),
				moonbeam::Body::TEXT,
			),
			SerdeFormError::UnsupportedFormType => {
				Response::bad_request().with_body("Unsupported form type", moonbeam::Body::TEXT)
			}
		}
	}
}

impl de::Error for SerdeFormError {
	fn custom<T: std::fmt::Display>(msg: T) -> Self {
		SerdeFormError::SerdeError(msg.to_string())
	}
}

impl std::fmt::Display for SerdeFormError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SerdeFormError::FormError(e) => write!(f, "Form error: {:?}", e),
			SerdeFormError::SerdeError(e) => write!(f, "Serde error: {}", e),
			SerdeFormError::UnsupportedFormType => write!(f, "Unsupported form type"),
		}
	}
}

impl std::error::Error for SerdeFormError {}

impl<'a, T: Deserialize<'a>> TryFrom<Request<'_, 'a>> for Form<T> {
	type Error = SerdeFormError;

	fn try_from(req: Request<'_, 'a>) -> Result<Self, Self::Error> {
		let raw_form = RawForm::try_from(req).map_err(SerdeFormError::FormError)?;
		let mut map = BTreeMap::new();

		match raw_form {
			RawForm::URLEncoded(p) => {
				let inner = p.into_inner();
				match inner {
					Cow::Borrowed(s) => {
						for (k, v) in AllParamIter::new(s) {
							map.entry(Cow::Borrowed(k))
								.or_insert_with(Vec::new)
								.push(Value::Text(Cow::Borrowed(v)));
						}
					}
					Cow::Owned(s) => {
						for (k, v) in AllParamIter::new(&s) {
							map.entry(Cow::Owned(k.to_string()))
								.or_insert_with(Vec::new)
								.push(Value::Text(Cow::Owned(v.to_string())));
						}
					}
				}
			}
			RawForm::Multipart(m) => {
				for (name, data) in m.iter() {
					if let Some(name) = name {
						let value = match data {
							FormData::Text(t) => Value::Text(Cow::Borrowed(t)),
							FormData::File {
								name,
								content_type,
								data,
							} => Value::File(File {
								name,
								content_type,
								data,
							}),
						};
						map.entry(Cow::Borrowed(name))
							.or_insert_with(Vec::new)
							.push(value);
					}
				}
			}
			_ => {
				return Err(SerdeFormError::UnsupportedFormType);
			}
		}

		let deserializer = FormDeserializer::new(map);
		T::deserialize(deserializer).map(Form)
	}
}

impl<'a, T: Deserialize<'a>, S> FromRequest<'_, 'a, S> for Form<T> {
	type Error = Response;

	async fn from_request(req: Request<'_, 'a>, _state: &'static S) -> Result<Self, Self::Error> {
		Form::try_from(req).map_err(|e| e.into())
	}
}

#[derive(Debug)]
enum Value<'a> {
	Text(Cow<'a, str>),
	File(File<'a>),
}

struct FormDeserializer<'a> {
	iter: std::collections::btree_map::IntoIter<Cow<'a, str>, Vec<Value<'a>>>,
	current_value: Option<Vec<Value<'a>>>,
}

impl<'a> FormDeserializer<'a> {
	fn new(map: BTreeMap<Cow<'a, str>, Vec<Value<'a>>>) -> Self {
		Self {
			iter: map.into_iter(),
			current_value: None,
		}
	}
}

impl<'de> de::Deserializer<'de> for FormDeserializer<'de> {
	type Error = SerdeFormError;

	fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		Err(de::Error::custom(
			"form deserialization only supported for structs",
		))
	}

	fn deserialize_struct<V>(
		self,
		_name: &'static str,
		_fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_map(self)
	}

	forward_to_deserialize_any! {
		bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
		option unit unit_struct newtype_struct seq tuple bytes byte_buf
		tuple_struct map enum identifier ignored_any
	}
}

impl<'de> de::MapAccess<'de> for FormDeserializer<'de> {
	type Error = SerdeFormError;

	fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
	where
		K: de::DeserializeSeed<'de>,
	{
		match self.iter.next() {
			Some((key, value)) => {
				self.current_value = Some(value);
				seed.deserialize(KeyDeserializer(key)).map(Some)
			}
			None => Ok(None),
		}
	}

	fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
	where
		V: de::DeserializeSeed<'de>,
	{
		let values = self
			.current_value
			.take()
			.ok_or_else(|| de::Error::custom("requested value without key"))?;
		seed.deserialize(ValuesDeserializer(values.into_iter()))
	}
}

struct KeyDeserializer<'a>(Cow<'a, str>);

impl<'de> de::Deserializer<'de> for KeyDeserializer<'de> {
	type Error = SerdeFormError;

	fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		Err(de::Error::custom("keys must be identifiers"))
	}

	fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0 {
			Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
			Cow::Owned(s) => visitor.visit_str(&s),
		}
	}

	forward_to_deserialize_any! {
		bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
		option unit unit_struct newtype_struct seq tuple struct bytes byte_buf
		tuple_struct map enum ignored_any
	}
}

struct ValuesDeserializer<'a>(std::vec::IntoIter<Value<'a>>);

macro_rules! impl_values_deserialize {
	($name:ident) => {
		fn $name<V>(self, visitor: V) -> Result<V::Value, Self::Error>
		where
			V: Visitor<'de>,
		{
			match self.0.last() {
				Some(v) => ValueDeserializer(v).$name(visitor),
				None => Err(de::Error::custom(
					"deserializing empty sequence but expected a value",
				)),
			}
		}
	};
}

impl<'de> de::Deserializer<'de> for ValuesDeserializer<'de> {
	type Error = SerdeFormError;

	impl_values_deserialize!(deserialize_any);
	impl_values_deserialize!(deserialize_u8);
	impl_values_deserialize!(deserialize_u16);
	impl_values_deserialize!(deserialize_u32);
	impl_values_deserialize!(deserialize_u64);
	impl_values_deserialize!(deserialize_u128);
	impl_values_deserialize!(deserialize_i8);
	impl_values_deserialize!(deserialize_i16);
	impl_values_deserialize!(deserialize_i32);
	impl_values_deserialize!(deserialize_i64);
	impl_values_deserialize!(deserialize_i128);
	impl_values_deserialize!(deserialize_f32);
	impl_values_deserialize!(deserialize_f64);
	impl_values_deserialize!(deserialize_bool);
	impl_values_deserialize!(deserialize_char);
	impl_values_deserialize!(deserialize_bytes);
	impl_values_deserialize!(deserialize_byte_buf);

	fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		eprintln!("seq");
		visitor.visit_seq(self)
	}

	fn deserialize_struct<V>(
		self,
		name: &'static str,
		fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0.last() {
			Some(v) => ValueDeserializer(v).deserialize_struct(name, fields, visitor),
			None => Err(de::Error::custom(
				"deserializing empty sequence but expected a value",
			)),
		}
	}

	fn deserialize_newtype_struct<V>(
		self,
		_name: &'static str,
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0.last() {
			Some(v) => visitor.visit_newtype_struct(ValueDeserializer(v)),
			None => Err(de::Error::custom(
				"deserializing empty sequence but expected a value",
			)),
		}
	}

	fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0.last() {
			Some(v) => visitor.visit_some(ValueDeserializer(v)),
			None => visitor.visit_none(),
		}
	}

	forward_to_deserialize_any! {
		str string
		unit unit_struct tuple
		tuple_struct map enum identifier ignored_any
	}
}

impl<'de> de::SeqAccess<'de> for ValuesDeserializer<'de> {
	type Error = SerdeFormError;

	fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
	where
		T: de::DeserializeSeed<'de>,
	{
		match self.0.next() {
			Some(v) => seed.deserialize(ValueDeserializer(v)).map(Some),
			None => Ok(None),
		}
	}
}

struct ValueDeserializer<'a>(Value<'a>);

macro_rules! impl_value_deserialize {
	($type:ty) => {
		paste! {
			fn [< deserialize_ $type >]<V>(self, visitor: V) -> Result<V::Value, Self::Error>
			where
				V: Visitor<'de>,
			{
				if let Value::Text(t) = &self.0 {
					if let Ok(v) = t.parse::<$type>() {
						return visitor.[<visit_ $type>](v);
					}
				}
				self.deserialize_any(visitor)
			}
		}
	};
}

impl<'de> de::Deserializer<'de> for ValueDeserializer<'de> {
	type Error = SerdeFormError;

	fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0 {
			Value::Text(t) => match t {
				Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
				Cow::Owned(s) => visitor.visit_string(s),
			},
			Value::File(f) => FileDeserializer::new(f).deserialize_any(visitor),
		}
	}

	fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_some(self)
	}

	fn deserialize_struct<V>(
		self,
		name: &'static str,
		fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		if name == "File" {
			match self.0 {
				Value::File(f) => {
					FileDeserializer::new(f).deserialize_struct(name, fields, visitor)
				}
				Value::Text(_) => self.deserialize_any(visitor),
			}
		} else {
			Err(de::Error::custom(
				"forms only support basic types and the special File struct",
			))
		}
	}

	fn deserialize_newtype_struct<V>(
		self,
		_name: &'static str,
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_newtype_struct(self)
	}

	fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		if let Value::Text(t) = &self.0 {
			match t.as_ref() {
				"true" | "on" | "yes" | "1" => return visitor.visit_bool(true),
				"false" | "off" | "no" | "0" => return visitor.visit_bool(false),
				_ => {}
			}
		}
		self.deserialize_any(visitor)
	}

	fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		if let Value::Text(t) = &self.0 {
			match t {
				Cow::Borrowed(s) => visitor.visit_char(
					s.chars()
						.next()
						.ok_or_else(|| de::Error::custom("Deserializing empty string to char"))?,
				),
				Cow::Owned(s) => visitor.visit_char(
					s.chars()
						.next()
						.ok_or_else(|| de::Error::custom("Deserializing empty string to char"))?,
				),
			}
		} else {
			self.deserialize_any(visitor)
		}
	}

	fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0 {
			Value::Text(t) => match t {
				Cow::Borrowed(s) => visitor.visit_borrowed_bytes(s.as_bytes()),
				Cow::Owned(s) => visitor.visit_byte_buf(s.into_bytes()),
			},
			Value::File(f) => visitor.visit_borrowed_bytes(f.data),
		}
	}

	fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		self.deserialize_bytes(visitor)
	}

	impl_value_deserialize!(u8);
	impl_value_deserialize!(u16);
	impl_value_deserialize!(u32);
	impl_value_deserialize!(u64);
	impl_value_deserialize!(u128);
	impl_value_deserialize!(i8);
	impl_value_deserialize!(i16);
	impl_value_deserialize!(i32);
	impl_value_deserialize!(i64);
	impl_value_deserialize!(i128);
	impl_value_deserialize!(f32);
	impl_value_deserialize!(f64);

	forward_to_deserialize_any! {
		str string
		unit unit_struct seq tuple
		tuple_struct map enum identifier ignored_any
	}
}

struct FileDeserializer<'a> {
	file: File<'a>,
	state: u8,
}

impl<'a> FileDeserializer<'a> {
	fn new(file: File<'a>) -> Self {
		Self { file, state: 0 }
	}
}

impl<'de> de::Deserializer<'de> for FileDeserializer<'de> {
	type Error = SerdeFormError;

	fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_map(self)
	}

	forward_to_deserialize_any! {
		bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
		struct option unit unit_struct seq tuple bytes byte_buf newtype_struct
		tuple_struct map enum identifier ignored_any
	}
}

impl<'de> de::MapAccess<'de> for FileDeserializer<'de> {
	type Error = SerdeFormError;

	fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
	where
		K: de::DeserializeSeed<'de>,
	{
		match self.state {
			0 => {
				self.state = 1;
				seed.deserialize("name".into_deserializer()).map(Some)
			}
			2 => {
				self.state = 3;
				seed.deserialize("content_type".into_deserializer())
					.map(Some)
			}
			4 => {
				self.state = 5;
				seed.deserialize("data".into_deserializer()).map(Some)
			}
			_ => Ok(None),
		}
	}

	fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
	where
		V: de::DeserializeSeed<'de>,
	{
		match self.state {
			1 => {
				self.state = 2;
				seed.deserialize(SimpleOptionDeserializer(self.file.name))
			}
			3 => {
				self.state = 4;
				seed.deserialize(SimpleOptionDeserializer(self.file.content_type))
			}
			5 => {
				self.state = 6;
				seed.deserialize(BorrowedBytesDeserializer::new(self.file.data))
			}
			_ => Err(de::Error::custom("expected value")),
		}
	}
}

struct SimpleOptionDeserializer<'a>(Option<&'a str>);

impl<'de> de::Deserializer<'de> for SimpleOptionDeserializer<'de> {
	type Error = SerdeFormError;

	fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		Err(de::Error::custom("Expected Option"))
	}

	fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match &self.0 {
			Some(_) => visitor.visit_some(self),
			None => visitor.visit_none(),
		}
	}

	fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		match self.0 {
			Some(v) => visitor.visit_borrowed_str(v),
			None => Err(de::Error::custom(
				"Attempting to deserialize string on empty option",
			)),
		}
	}

	fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		self.deserialize_str(visitor)
	}

	forward_to_deserialize_any! {
		bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
		unit unit_struct newtype_struct seq tuple bytes byte_buf struct
		tuple_struct map enum identifier ignored_any
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use moonbeam::Header;
	use serde::Deserialize;

	#[derive(Debug, Deserialize, PartialEq)]
	struct User<'a> {
		id: u32,
		#[serde(borrow)]
		name: Cow<'a, str>,
		active: bool,
	}

	#[derive(Debug, Deserialize, PartialEq)]
	struct UserOwned {
		id: u32,
		name: String,
		active: bool,
	}

	#[test]
	fn test_form_urlencoded_get() {
		use moonbeam::http::Request;
		let req = Request::new("GET", "/test?id=42&name=Jens&active=true", &[], &[]);
		let Form(user): Form<User> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens");
		assert!(matches!(user.name, Cow::Borrowed(_)));
		assert!(user.active);
	}

	#[test]
	fn test_form_urlencoded_borrowed() {
		use moonbeam::http::Request;
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"application/x-www-form-urlencoded",
			}],
			b"id=42&name=Jens&active=true",
		);
		let Form(user): Form<User> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens");
		assert!(matches!(user.name, Cow::Borrowed(_)));
		assert!(user.active);
	}

	#[test]
	fn test_form_urlencoded_owned() {
		use moonbeam::http::Request;
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"application/x-www-form-urlencoded",
			}],
			b"id=42&name=Jens%20&active=true",
		);
		let Form(user): Form<User> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens ");
		assert!(matches!(user.name, Cow::Owned(_)));
		assert!(user.active);
	}

	#[test]
	fn test_form_urlencoded_always_owned() {
		use moonbeam::http::Request;
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"application/x-www-form-urlencoded",
			}],
			b"id=42&name=Jens%20&active=true",
		);
		let Form(user): Form<UserOwned> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens ");
		assert!(user.active);
	}

	#[test]
	fn test_form_multipart() {
		use moonbeam::http::Request;
		let body = b"--boundary\r\n\
					Content-Disposition: form-data; name=\"id\"\r\n\
					\r\n\
					42\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"name\"\r\n\
					\r\n\
					Jens\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"active\"\r\n\
					\r\n\
					yes\r\n\
					--boundary--";
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"multipart/form-data; boundary=boundary",
			}],
			body,
		);
		let Form(user): Form<User> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens");
		assert!(matches!(user.name, Cow::Borrowed(_)));
		assert!(user.active);
	}

	#[test]
	fn test_form_multipart_owned() {
		use moonbeam::http::Request;
		let body = b"--boundary\r\n\
					Content-Disposition: form-data; name=\"id\"\r\n\
					\r\n\
					42\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"name\"\r\n\
					\r\n\
					Jens\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"active\"\r\n\
					\r\n\
					yes\r\n\
					--boundary--";
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"multipart/form-data; boundary=boundary",
			}],
			body,
		);
		let Form(user): Form<UserOwned> = Form::try_from(req).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens");
		assert!(user.active);
	}

	#[test]
	fn test_form_sequence() {
		#[derive(Deserialize)]
		struct Multiple<'a> {
			#[serde(borrow)]
			a: Vec<&'a str>,
		}
		#[derive(Deserialize)]
		struct Single<'a> {
			a: &'a str,
		}
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"application/x-www-form-urlencoded",
			}],
			b"a=1&a=2&a=3",
		);

		let Form(s): Form<Single> = Form::try_from(req).unwrap();
		assert_eq!(s.a, "3");

		let Form(m): Form<Multiple> = Form::try_from(req).unwrap();
		assert_eq!(m.a, vec!["1", "2", "3"]);
	}

	#[test]
	fn test_form_file_borrowed() {
		#[derive(Deserialize)]
		struct Upload<'a> {
			title: &'a str,
			file: File<'a>,
		}
		use moonbeam::http::Request;
		let body = b"--boundary\r\n\
					Content-Disposition: form-data; name=\"title\"\r\n\
					\r\n\
					My File\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
					Content-Type: text/plain\r\n\
					\r\n\
					Hello World\r\n\
					--boundary--";
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"multipart/form-data; boundary=boundary",
			}],
			body,
		);
		let Form(u): Form<Upload> = Form::try_from(req).unwrap();
		assert_eq!(u.title, "My File");
		assert_eq!(u.file.name, Some("test.txt"));
		assert_eq!(u.file.content_type, Some("text/plain"));
		assert_eq!(u.file.data, b"Hello World");

		// Verify zero-copy: the pointers should be within the body range
		let body_range = body.as_ptr()..unsafe { body.as_ptr().add(body.len()) };
		assert!(body_range.contains(&u.title.as_ptr()));
		assert!(body_range.contains(&u.file.data.as_ptr()));
	}

	#[test]
	fn test_newtype_struct() {
		#[derive(Debug, Deserialize, PartialEq)]
		struct Id(u32);

		#[derive(Debug, Deserialize, PartialEq)]
		struct User<'a> {
			id: Id,
			#[serde(borrow)]
			name: Cow<'a, str>,
			active: bool,
		}

		use moonbeam::http::Request;
		let req = Request::new("GET", "/test?id=42&name=Jens&active=true", &[], &[]);
		let Form(user): Form<User> = Form::try_from(req).unwrap();

		assert_eq!(user.id, Id(42));
		assert_eq!(user.name, "Jens");
		assert!(matches!(user.name, Cow::Borrowed(_)));
		assert!(user.active);
	}

	#[test]
	fn test_bytes() {
		#[derive(Deserialize)]
		struct Upload<'a> {
			title: &'a [u8],
			file: &'a [u8],
		}
		use moonbeam::http::Request;
		let body = b"--boundary\r\n\
					Content-Disposition: form-data; name=\"title\"\r\n\
					\r\n\
					My File\r\n\
					--boundary\r\n\
					Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
					Content-Type: text/plain\r\n\
					\r\n\
					Hello World\r\n\
					--boundary--";
		let req = Request::new(
			"POST",
			"/test",
			&[Header {
				name: "Content-Type",
				value: b"multipart/form-data; boundary=boundary",
			}],
			body,
		);
		let Form(u): Form<Upload> = Form::try_from(req).unwrap();
		assert_eq!(u.title, b"My File");
		assert_eq!(u.file, b"Hello World");
	}
}
