use std::fs;
use serde_json::json;
use websrv::{WebSrv, StatusCode, Method, Route, Request};

static ROUTES: &'static [Route] = &[
    Route {path: "/hello", method: Method::GET, func: hello_world},
    Route {path: "/hello_rust", method: Method::GET, func: hello_rust},
    Route {path: "/forbidden", method: Method::GET, func: forbidden},
];

fn main() {
    env_logger::init();
    let websrv = WebSrv::new("127.0.0.1:7878", 4);

    websrv.run(ROUTES);
}

fn hello_world(_: &Request) -> (StatusCode, String) {
    let response = fs::read_to_string("templates/hello.html").unwrap();
    (StatusCode::Ok, response)
}

fn hello_rust(request: &Request) -> (StatusCode, String) {
    (StatusCode::Created, json!({
        "message": "Hello from rust!",
        "method": request.method,
        "uri": request.uri,
        "http_ver": request.http_ver
    }).to_string())
}

fn forbidden(_: &Request) -> (StatusCode, String) {
    let response = fs::read_to_string("templates/403.html").unwrap();
    (StatusCode::Forbidden, response)
}