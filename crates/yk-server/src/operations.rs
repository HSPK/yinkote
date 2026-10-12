//! HTTP routing and MCP discovery share these typed operation declarations.

use std::future::Future;

use axum::extract::{Path, Query, State};
use axum::handler::Handler;
use axum::http::{HeaderMap, Method};
use axum::routing::{on, MethodFilter};
use axum::{Json, Router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::state::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    Read,
    Write,
    Dangerous,
}

#[derive(Default)]
pub struct Inputs {
    path: Option<Value>,
    query: Option<Value>,
    body: Option<Value>,
    version_header: bool,
}

pub trait Input {
    fn describe(inputs: &mut Inputs);
}

fn schema<T: JsonSchema>() -> Value {
    let settings = schemars::generate::SchemaSettings::draft07().with(|settings| {
        settings.inline_subschemas = true;
    });
    serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("JSON schema is serializable")
}

impl Input for State<App> {
    fn describe(_: &mut Inputs) {}
}
impl<T: JsonSchema> Input for Path<T> {
    fn describe(inputs: &mut Inputs) {
        inputs.path = Some(schema::<T>());
    }
}
impl<T: JsonSchema> Input for Query<T> {
    fn describe(inputs: &mut Inputs) {
        inputs.query = Some(schema::<T>());
    }
}
impl<T: JsonSchema> Input for Json<T> {
    fn describe(inputs: &mut Inputs) {
        inputs.body = Some(schema::<T>());
    }
}
impl Input for HeaderMap {
    fn describe(inputs: &mut Inputs) {
        inputs.version_header = true;
    }
}

pub trait DescribedHandler<T>: Handler<T, App> {
    fn inputs() -> Inputs;
}

macro_rules! describe_handler {
    ($($arg:ident),*) => {
        impl<F, M, Fut, $($arg,)*> DescribedHandler<(M, $($arg,)*)> for F
        where
            F: Handler<(M, $($arg,)*), App> + FnOnce($($arg),*) -> Fut,
            Fut: Future,
            $($arg: Input,)*
        {
            fn inputs() -> Inputs {
                let mut inputs = Inputs::default();
                $($arg::describe(&mut inputs);)*
                inputs
            }
        }
    };
}
describe_handler!(A);
describe_handler!(A, B);
describe_handler!(A, B, C);
describe_handler!(A, B, C, D);
describe_handler!(A, B, C, D, E);

impl<F, Fut> DescribedHandler<((),)> for F
where
    F: Handler<((),), App> + FnOnce() -> Fut,
    Fut: Future,
{
    fn inputs() -> Inputs {
        Inputs::default()
    }
}

#[derive(Clone)]
pub struct Operation {
    pub name: &'static str,
    pub description: &'static str,
    pub method: Method,
    pub path: &'static str,
    pub access: Access,
    pub input_schema: Value,
}

impl Operation {
    fn new(
        name: &'static str,
        description: &'static str,
        method: Method,
        path: &'static str,
        access: Access,
        inputs: Inputs,
    ) -> Self {
        let mut properties = Map::new();
        let mut required = Vec::new();
        let names: Vec<_> = path
            .split('/')
            .filter_map(|part| part.strip_prefix(':'))
            .collect();
        if !names.is_empty() {
            let path_schema = inputs.path.expect("path extractor is required");
            let tuple = path_schema.get("items").and_then(Value::as_array);
            let mut fields = Map::new();
            let mut path_required = Vec::new();
            for (index, name) in names.iter().enumerate() {
                let field = tuple
                    .map(|parts| parts[index].clone())
                    .unwrap_or_else(|| path_schema.clone());
                fields.insert((*name).to_string(), field);
                if *name != "lib" {
                    path_required.push(*name);
                }
            }
            if !path_required.is_empty() {
                required.push("path");
            }
            properties.insert(
                "path".into(),
                json!({
                    "type":"object", "properties": fields, "required":path_required,
                    "additionalProperties":false,
                    "description":"Route parameters. lib defaults to the current library."
                }),
            );
        }
        if let Some(mut query) = inputs.query {
            if let Some(object) = query.as_object_mut() {
                object.insert("additionalProperties".into(), Value::Bool(false));
            }
            properties.insert("query".into(), query);
        }
        if let Some(body) = inputs.body {
            properties.insert("body".into(), body);
            required.push("body");
        }
        if inputs.version_header {
            properties.insert("ifVersion".into(), json!({
                "type":"integer", "description":"Optimistic concurrency version, when supported by this operation."
            }));
        }
        Self {
            name,
            description,
            method,
            path,
            access,
            input_schema: json!({
                "type":"object", "properties":properties, "required":required, "additionalProperties":false
            }),
        }
    }
}

#[derive(Default)]
pub struct Api {
    pub router: Router<App>,
    pub operations: Vec<Operation>,
}

impl Api {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route<H, T>(
        mut self,
        path: &'static str,
        method: Method,
        name: &'static str,
        description: &'static str,
        access: Access,
        handler: H,
    ) -> Self
    where
        H: DescribedHandler<T>,
        T: 'static,
    {
        let filter = MethodFilter::try_from(method.clone()).expect("supported HTTP method");
        self.operations.push(Operation::new(
            name,
            description,
            method,
            path,
            access,
            H::inputs(),
        ));
        self.router = self.router.route(path, on(filter, handler));
        self
    }

    pub fn merge(mut self, other: Self) -> Self {
        self.router = self.router.merge(other.router);
        self.operations.extend(other.operations);
        self
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arguments {
    #[serde(default)]
    pub path: Map<String, Value>,
    #[serde(default)]
    pub query: Map<String, Value>,
    pub body: Option<Value>,
    #[serde(rename = "ifVersion")]
    pub if_version: Option<i64>,
}

#[macro_export]
macro_rules! api_routes {
    ($( $method:ident $path:literal => $handler:ident, $name:literal, $access:ident, $description:literal; )*) => {
        $crate::operations::Api::new()
        $(.route($path, axum::http::Method::$method, $name, $description,
            $crate::operations::Access::$access, $handler))*
    };
}
