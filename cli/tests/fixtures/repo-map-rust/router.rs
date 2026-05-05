pub mod handlers {
    pub fn ping() -> &'static str {
        "pong"
    }

    pub fn echo(s: &str) -> String {
        s.to_string()
    }
}

pub struct Router {
    pub routes: Vec<Route>,
}

pub struct Route {
    pub path: String,
}

pub fn build_router() -> Router {
    Router { routes: Vec::new() }
}
