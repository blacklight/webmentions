//! axum adapter tests — mirrors the Flask/FastAPI/Tornado adapter
//! coverage in `tests/python`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use webmentions_sdk::server::axum::bind_webmentions;
use webmentions_sdk::*;

#[derive(Default)]
struct MemoryStorage {
    mentions: Mutex<Vec<Webmention>>,
}

impl WebmentionsStorage for MemoryStorage {
    fn store_webmention(&self, mention: &Webmention) -> Result<()> {
        self.mentions.lock().unwrap().push(mention.clone());
        Ok(())
    }

    fn delete_webmention(
        &self,
        source: &str,
        target: &str,
        direction: WebmentionDirection,
    ) -> Result<()> {
        self.mentions
            .lock()
            .unwrap()
            .retain(|m| !(m.source == source && m.target == target && m.direction == direction));
        Ok(())
    }

    fn retrieve_webmentions(
        &self,
        resource: &str,
        direction: WebmentionDirection,
    ) -> Result<Vec<Webmention>> {
        Ok(self
            .mentions
            .lock()
            .unwrap()
            .iter()
            .filter(|m| {
                m.direction == direction
                    && if direction == WebmentionDirection::Out {
                        m.source == resource
                    } else {
                        m.target == resource
                    }
            })
            .cloned()
            .collect())
    }
}

fn fetcher_with(url: &str, body: String) -> Fetcher {
    let body = body.to_string();
    let url = url.to_string();
    Arc::new(move |req: &FetchRequest| {
        Ok(FetchResponse {
            status: 200,
            headers: HashMap::new(),
            body: if req.url == url {
                body.as_bytes().to_vec()
            } else {
                Vec::new()
            },
            url: req.url.clone(),
        })
    })
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn webmention_post_get_and_link_header() {
    let target = "https://me.example/article";
    let fetcher = fetcher_with(
        "https://remote.example/post",
        format!("<a href=\"{target}\">t</a>"),
    );
    let handler = Arc::new(WebmentionsHandler::new(
        Arc::new(MemoryStorage::default()),
        HandlerOptions {
            fetcher: Some(fetcher),
            ..Default::default()
        },
    ));

    let app = Router::new()
        .route("/", get(|| async { "<html>hi</html>" }))
        .route(
            "/text",
            get(|| async { ([("content-type", "text/plain")], "hi") }),
        );
    let (app, _endpoints) = bind_webmentions(app, handler.clone(), "/webmentions");

    // POST an incoming webmention.
    let req = Request::post("/webmentions")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "source=https%3A%2F%2Fremote.example%2Fpost&target=https%3A%2F%2Fme.example%2Farticle",
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("\"ok\""));

    // GET stored mentions.
    let req = Request::get(format!(
        "/webmentions?resource={}&direction=incoming",
        target.replace("https://", "https%3A%2F%2F")
    ))
    .body(Body::empty())
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("remote.example"));

    // GET without params → 400.
    let req = Request::get("/webmentions").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // A text/* response advertises the endpoint via Link header.
    let req = Request::get("/text").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let link = resp
        .headers()
        .get("link")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        link.contains("</webmentions>; rel=\"webmention\""),
        "link: {link}"
    );
}
