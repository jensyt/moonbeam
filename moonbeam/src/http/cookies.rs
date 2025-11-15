pub struct Cookies {

}

impl Cookies {
	pub fn new(_cookies: Option<&[u8]>) -> Self {
		Cookies {  }
	}

	pub fn find(&self, _cookie: &str) -> Option<&[u8]> {
		None
	}
}
