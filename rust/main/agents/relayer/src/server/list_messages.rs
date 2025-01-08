use axum::{
    extract::{Query, State},
    routing, Router,
};
use derive_new::new;
use hyperlane_core::{QueueOperation, H256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::msg::op_queue::OperationPriorityQueue;

const LIST_OPERATIONS_API_BASE: &str = "/list_operations";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ListOperationsRequest {
    destination_domain: u32,
}

impl PartialEq<QueueOperation> for &ListOperationsRequest {
    fn eq(&self, other: &QueueOperation) -> bool {
        self.destination_domain == other.destination_domain().id()
    }
}

#[derive(new, Clone)]
pub struct ListOperationsApi {
    op_queues: HashMap<u32, OperationPriorityQueue>,
}

async fn list_operations(
    State(queues): State<HashMap<u32, OperationPriorityQueue>>,
    Query(request): Query<ListOperationsRequest>,
) -> String {
    let domain = request.destination_domain;
    let Some(op_queue) = queues.get(&domain) else {
        return format!("No queue found for domain {}", domain);
    };
    format_queue(op_queue.clone()).await
}

#[derive(Debug, Serialize)]
struct OperationWithId<'a> {
    id: H256,
    operation: &'a QueueOperation,
}

impl<'a> OperationWithId<'a> {
    fn new(operation: &'a QueueOperation) -> Self {
        Self {
            id: operation.id(),
            operation,
        }
    }
}

pub async fn format_queue(queue: OperationPriorityQueue) -> String {
    let res: Result<Vec<Value>, _> = queue
        .lock()
        .await
        .iter()
        .map(|reverse| serde_json::to_value(OperationWithId::new(&reverse.0)))
        .collect();
    match res.and_then(|v| serde_json::to_string_pretty(&v)) {
        Ok(s) => s,
        Err(e) => format!("Error formatting queue: {}", e),
    }
}

impl ListOperationsApi {
    pub fn router(&self) -> Router {
        Router::new()
            .route("/", routing::get(list_operations))
            .with_state(self.op_queues.clone())
    }

    pub fn get_route(&self) -> (&'static str, Router) {
        (LIST_OPERATIONS_API_BASE, self.router())
    }
}

#[cfg(test)]
mod tests {
    use crate::msg::op_queue::{
        test::{dummy_metrics_and_label, MockPendingOperation},
        OpQueue,
    };

    use super::*;
    use axum::http::StatusCode;
    use hyperlane_core::KnownHyperlaneDomain;
    use std::{cmp::Reverse, net::SocketAddr, sync::Arc};
    use tokio::sync::{self, Mutex};

    const DUMMY_DOMAIN: KnownHyperlaneDomain = KnownHyperlaneDomain::Arbitrum;

    fn setup_test_server() -> (SocketAddr, OperationPriorityQueue) {
        let (metrics, queue_metrics_label) = dummy_metrics_and_label();
        let broadcaster = sync::broadcast::Sender::new(100);

        let op_queue = OpQueue::new(
            metrics.clone(),
            queue_metrics_label.clone(),
            Arc::new(Mutex::new(broadcaster.subscribe())),
        );
        let mut op_queues_map = HashMap::new();
        op_queues_map.insert(DUMMY_DOMAIN as u32, op_queue.queue.clone());

        let list_operations_api = ListOperationsApi::new(op_queues_map);
        let (path, router) = list_operations_api.get_route();

        let app = Router::new().nest(path, router);

        // Running the app in the background using a test server
        let server =
            axum::Server::bind(&"127.0.0.1:0".parse().unwrap()).serve(app.into_make_service());
        let addr = server.local_addr();
        tokio::spawn(server);

        (addr, op_queue.queue.clone())
    }

    #[tokio::test]
    async fn test_message_id_retry() {
        let (addr, op_queue) = setup_test_server();
        let id_1 = "0x1acbee9798118b11ebef0d94b0a2936eafd58e3bfab91b05da875825c4a1c39b";
        let id_2 = "0x51e7be221ce90a49dee46ca0d0270c48d338a7b9d85c2a89d83fac0816571914";
        let sender_address = "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08";
        let recipient_address =
            "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08";
        let dummy_operation_1 = Box::new(
            MockPendingOperation::new(1, DUMMY_DOMAIN.into())
                .with_id(id_1)
                .with_sender_address(sender_address)
                .with_recipient_address(recipient_address),
        ) as QueueOperation;
        let dummy_operation_2 = Box::new(
            MockPendingOperation::new(2, DUMMY_DOMAIN.into())
                .with_id(id_2)
                .with_sender_address(sender_address)
                .with_recipient_address(recipient_address),
        ) as QueueOperation;

        // The reason there already is an id inside `operation` here is because it's a field on `MockPendingOperation` - that field is
        // missing on `PendingMessage` because it's derived, hence the need to hence the need to have it explicitly serialized alongside the operation.
        let expected_response = r#"[
  {
    "id": "0x1acbee9798118b11ebef0d94b0a2936eafd58e3bfab91b05da875825c4a1c39b",
    "operation": {
      "destination_domain": {
        "Known": "Arbitrum"
      },
      "destination_domain_id": 42161,
      "id": "0x1acbee9798118b11ebef0d94b0a2936eafd58e3bfab91b05da875825c4a1c39b",
      "origin_domain_id": 0,
      "recipient_address": "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08",
      "seconds_to_next_attempt": 1,
      "sender_address": "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08",
      "type": "MockPendingOperation"
    }
  },
  {
    "id": "0x51e7be221ce90a49dee46ca0d0270c48d338a7b9d85c2a89d83fac0816571914",
    "operation": {
      "destination_domain": {
        "Known": "Arbitrum"
      },
      "destination_domain_id": 42161,
      "id": "0x51e7be221ce90a49dee46ca0d0270c48d338a7b9d85c2a89d83fac0816571914",
      "origin_domain_id": 0,
      "recipient_address": "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08",
      "seconds_to_next_attempt": 2,
      "sender_address": "0x586d41b02fb35df0f84ecb2b73e076b40c929ee3e1ceeada9a078aa7b46d3b08",
      "type": "MockPendingOperation"
    }
  }
]"#;
        op_queue.lock().await.push(Reverse(dummy_operation_1));
        op_queue.lock().await.push(Reverse(dummy_operation_2));

        // Send a GET request to the server
        let response = reqwest::get(format!(
            "http://{}{}?destination_domain={}",
            addr, LIST_OPERATIONS_API_BASE, DUMMY_DOMAIN as u32
        ))
        .await
        .unwrap();

        // Check that the response status code is OK
        assert_eq!(response.status(), StatusCode::OK);

        let response_text = response.text().await.unwrap();
        assert_eq!(response_text, expected_response);
    }
}
