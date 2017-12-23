#![deny(warnings)]
extern crate futures;
extern crate hyper;
extern crate pretty_env_logger;
extern crate tokio_core;

use futures::{Future, Stream};

use hyper::{Body, Chunk, Client, Get, Post, StatusCode};
use hyper::error::Error;
use hyper::header::ContentLength;
use hyper::server::{Http, Service, Request, Response};

use std::ascii::AsciiExt;

static NOTFOUND: &[u8] = b"Not Found";
static URL: &str = "http://127.0.0.1:1337/web_api";
static LOWERCASE: &[u8] = b"i am a lower case string";

pub type ResponseStream = Box<Stream<Item=Chunk, Error=Error>>;

struct ResponseExamples;

impl Service for ResponseExamples {
    type Request = Request;
    type Response = Response<ResponseStream>;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Get, "/") | (&Get, "/index.html") => {
                // Run a web query against the web api below
                let core = tokio_core::reactor::Core::new().unwrap();
                let client = Client::configure().build(&core.handle());
                let mut req = Request::new(Post, URL.parse().unwrap());
                req.set_body(LOWERCASE);
                let web_res_future = client.request(req);

                Box::new(web_res_future.map(|web_res| {
                    let body: ResponseStream = Box::new(web_res.body().concat2().map(|b| {
                        Chunk::from(format!("before: '{:?}'\nafter: '{:?}'", LOWERCASE, b))
                    }).into_stream());
                    Response::new().with_body(body)
                }))
            },
            (&Post, "/web_api") => {
                // A web api to run against. Simple upcasing of the body.
                let body: ResponseStream = Box::new(req.body().map(|chunk| {
                    let upper = chunk.iter().map(|byte| byte.to_ascii_uppercase())
                        .collect::<Vec<u8>>();
                    Chunk::from(upper)
                }));
                Box::new(futures::future::ok(Response::new().with_body(body)))
            },
            _ => {
                let body: ResponseStream = Box::new(Body::from(NOTFOUND));
                Box::new(futures::future::ok(Response::new()
                                             .with_status(StatusCode::NotFound)
                                             .with_header(ContentLength(NOTFOUND.len() as u64))
                                             .with_body(body)))
            }
        }
    }

}


fn main() {
    pretty_env_logger::init().unwrap();
    let addr = "127.0.0.1:1337".parse().unwrap();

    let mut server = Http::new().bind(&addr, || Ok(ResponseExamples)).unwrap();
    server.no_proto();
    println!("Listening on http://{} with 1 thread.", server.local_addr().unwrap());
    server.run().unwrap();
}
