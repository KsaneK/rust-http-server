use std::io::prelude::*;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use serde::Serialize;
use log::{debug, info};


pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

enum Message {
    NewJob(Job),
    Terminate,
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();

            match message {
                Message::NewJob(job) => {
                    debug!("Worker {} got a job; executing.", id);

                    job();
                }
                Message::Terminate => {
                    debug!("Worker {} was told to terminate.", id);

                    break;
                }
            }
        });
        Worker {
            id,
            thread: Some(thread),
        }
    }
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            workers.push(Worker::new(i, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender.send(Message::NewJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        debug!("Sending terminate message to all workers.");

        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        debug!("Shutting down all workers.");

        for worker in &mut self.workers {
            debug!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

pub struct WebSrv {
    addr: String,
    workers: usize,
}

impl WebSrv {
    pub fn new(addr: &str, workers: usize) -> WebSrv {
        WebSrv {
            addr: String::from(addr),
            workers,
        }
    }

    pub fn run(&self, routes: &'static [Route]) {
        let listener = TcpListener::bind(self.addr.to_string()).unwrap();
        let pool = ThreadPool::new(self.workers);
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            pool.execute(move || {
                WebSrv::handle_connection(stream, routes);
            });
        }
    }

    fn handle_connection(mut stream: TcpStream, routes: &'static [Route]) {
        let mut buffer = [0; 4096];
        stream.read(&mut buffer).unwrap();
        let request_body = String::from_utf8_lossy(&buffer[..]);
        let request = Request::from_str(&request_body);
        if request.is_none() {
            return;
        }
        let request = request.unwrap();

        for route in routes {
            if route.path == request.uri && request.method == route.method {
                let (status_code, response) = (route.func)(&request);
                let mut response_request = String::new();

                response_request.push_str(format!("HTTP/1.1 {}\r\n", status_code.text()).as_str());

                if response.len() > 0 {
                    response_request.push_str(format!("Content-Length: {}\r\n\r\n", response.len().to_string()).as_str());
                    response_request.push_str(response.as_str());
                }


                stream.write(response_request.as_bytes()).unwrap();
                stream.flush().unwrap();

                info!("Request from {}: {} - {}", stream.local_addr().unwrap(), request.uri, status_code.text());
                return;
            }
        }

        // Route not found
        let result = std::fs::read_to_string("templates/404.html").unwrap();
        let result = format!("HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n{}", result.len(), result);
        stream.write(result.as_bytes()).unwrap();
        stream.flush().unwrap();
        info!("Request from {}: {} - {}", stream.local_addr().unwrap(), request.uri, StatusCode::NotFound.text());
    }
}

pub struct Request {
    pub http_ver: String,
    pub method: Method,
    pub uri: String,
    pub headers: Vec<Header>,
    pub body: String,
}

impl Request {
    fn from_str(body: &std::borrow::Cow<'_, str>) -> Option<Request> {
        let mut lines = body.lines();
        let mut firstline = lines.next().unwrap().split_whitespace();
        let method = Method::from_str(firstline.next());
        if method.is_none() {
            return None;
        }
        let method = method.unwrap();
        let uri = firstline.next().unwrap();
        let http_ver = firstline.next().unwrap();
        let mut headers = vec![];

        let mut body = String::new();
        let mut parsing_headers = true;
        for line in lines {
            if line.chars().next().is_none() {
                parsing_headers = false;
                continue;
            }

            match parsing_headers {
                true => {
                    let mut kv = line.split(": ");
                    headers.push(Header::new(kv.next().unwrap(), kv.next().unwrap()));
                },
                false => {
                    body.push_str(line);
                }
            }

        }

        Some(Request {http_ver: String::from(http_ver), method, uri: String::from(uri), headers, body})
    }
}

#[derive(Debug)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Header {
    fn new(key: &str, val: &str) -> Header {
        Header {key: String::from(key), value: String::from(val)}
    }
}

pub struct Route {
    pub path: &'static str,
    pub method: Method,
    pub func: fn(request: &Request) -> (StatusCode, String),
}

impl Route {
    pub fn new(path: &'static str, method: Method, func: fn(request: &Request) -> (StatusCode, String)) -> Route {
        Route { path, method, func }
    }
}

#[derive(Debug)]
pub enum StatusCode {
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
}

impl StatusCode {
    fn text(&self) -> &'static str {
        match self {
            StatusCode::Ok => "200 OK",
            StatusCode::Created => "201 Created",
            StatusCode::Accepted => "202 Accepted",
            StatusCode::NoContent => "204 No Content",
            StatusCode::BadRequest => "400 Bad Request",
            StatusCode::Unauthorized => "401 Unauthorized",
            StatusCode::Forbidden => "403 Forbidden",
            StatusCode::NotFound => "404 Not Found",
        }
    }
}

#[derive(PartialEq)]
#[derive(Serialize)]
pub enum Method {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE
}

impl Method {
    pub fn from_str(method: Option<&str>) -> Option<Method> {
        match method {
            Some("GET") => Some(Method::GET),
            Some("POST") => Some(Method::POST),
            Some("PUT") => Some(Method::PUT),
            Some("PATCH") => Some(Method::PATCH),
            Some("DELETE") => Some(Method::DELETE),
            _ => None
        }
    }
}