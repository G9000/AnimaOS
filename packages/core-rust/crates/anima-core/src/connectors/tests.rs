use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn descriptor(id: &str) -> ConnectorDescriptor {
    ConnectorDescriptor {
        protocol_id: id.into(),
        contract_revision: CONNECTOR_CONTRACT_REVISION,
        display_name: "Invented transport".into(),
        transport_kind: ConnectorTransportKind::Polling,
        operations: vec![
            ConnectorOperation::Identify,
            ConnectorOperation::Receive,
            ConnectorOperation::Send,
        ],
        source: "fixture".into(),
    }
}

#[test]
fn registry_owns_sorted_metadata_and_rejects_ambiguity() {
    let mut source = vec![descriptor("zeta"), descriptor("alpha")];
    let registry = ConnectorRegistry::new(source.clone()).unwrap();
    source[0].display_name = "changed".into();
    assert_eq!(
        registry
            .list()
            .map(|d| d.protocol_id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert_eq!(
        registry.get("zeta").unwrap().display_name,
        "Invented transport"
    );
    assert!(registry.get("missing").is_none());
    assert_eq!(
        ConnectorRegistry::new([descriptor("same"), descriptor("same")]).unwrap_err(),
        ConnectorRegistryError::DuplicateProtocolId
    );
    for invalid in ["", "Upper", "hidden\n", "a/b", "λ"] {
        assert_eq!(
            ConnectorRegistry::new([descriptor(invalid)]).unwrap_err(),
            ConnectorRegistryError::InvalidProtocolId
        );
    }
    let mut invalid = descriptor("valid");
    invalid.contract_revision = 2;
    assert_eq!(
        ConnectorRegistry::new([invalid]).unwrap_err(),
        ConnectorRegistryError::UnsupportedRevision
    );
    let mut invalid = descriptor("valid");
    invalid.operations.push(ConnectorOperation::Send);
    assert_eq!(
        ConnectorRegistry::new([invalid]).unwrap_err(),
        ConnectorRegistryError::DuplicateOperation
    );
    for (field, value, expected) in [
        (
            "name",
            "n".repeat(129),
            ConnectorRegistryError::InvalidDisplayName,
        ),
        (
            "source",
            "s".repeat(65),
            ConnectorRegistryError::InvalidSource,
        ),
        (
            "source",
            "private\nlabel".into(),
            ConnectorRegistryError::InvalidSource,
        ),
    ] {
        let mut invalid = descriptor("valid");
        if field == "name" {
            invalid.display_name = value;
        } else {
            invalid.source = value;
        }
        assert_eq!(ConnectorRegistry::new([invalid]).unwrap_err(), expected);
    }
}

#[test]
fn request_bounds_count_scalars_and_preserve_message_body() {
    let mut request = ConnectorSendRequest {
        destination: "chat".into(),
        attempt_id: "attempt".into(),
        text: " \n\t\0".into(),
    };
    assert!(request.validate().is_ok());
    request.text = "λ".repeat(MAX_CONNECTOR_TEXT_CHARS);
    assert!(request.validate().is_ok());
    request.text.push('λ');
    assert_eq!(request.validate().unwrap_err().field, "text");
    request.text.clear();
    assert_eq!(request.validate().unwrap_err().field, "text");
    request.text = "body".into();
    request.destination = "x".repeat(MAX_CONNECTOR_DESTINATION_CHARS + 1);
    assert_eq!(request.validate().unwrap_err().field, "destination");
    request.destination = "chat".into();
    request.attempt_id = "hidden\nvalue".into();
    let error = request.validate().unwrap_err();
    assert_eq!(error.field, "attempt_id");
    assert!(!error.to_string().contains("hidden"));
    assert!(ConnectorReceiveRequest {
        cursor: String::new()
    }
    .validate()
    .is_ok());
    assert!(ConnectorReceiveRequest {
        cursor: "λ".repeat(MAX_CONNECTOR_CURSOR_CHARS)
    }
    .validate()
    .is_ok());
    assert!(ConnectorReceiveRequest {
        cursor: "x".repeat(MAX_CONNECTOR_CURSOR_CHARS + 1)
    }
    .validate()
    .is_err());
}

#[derive(Debug)]
struct FixtureFailure;
impl fmt::Display for FixtureFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fixture failure")
    }
}
impl Error for FixtureFailure {}
impl ConnectorFailure for FixtureFailure {
    fn kind(&self) -> ConnectorErrorKind {
        ConnectorErrorKind::Uncertain
    }
}
struct NativeIdentity(u32);
struct NativeBatch {
    cursor: String,
}
struct NativeReceipt {
    attempt: String,
}
struct FixtureTransport(AtomicUsize);
#[async_trait]
impl ConnectorTransport for FixtureTransport {
    type Identity = NativeIdentity;
    type InboundBatch = NativeBatch;
    type SendReceipt = NativeReceipt;
    type Error = FixtureFailure;
    async fn identify(&self) -> Result<NativeIdentity, FixtureFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(NativeIdentity(42))
    }
    async fn receive(
        &self,
        request: ConnectorReceiveRequest,
    ) -> Result<NativeBatch, FixtureFailure> {
        Ok(NativeBatch {
            cursor: request.cursor,
        })
    }
    async fn send(&self, request: ConnectorSendRequest) -> Result<NativeReceipt, FixtureFailure> {
        Ok(NativeReceipt {
            attempt: request.attempt_id,
        })
    }
}

#[test]
fn passive_registry_and_native_async_transport_need_no_host() {
    let transport = FixtureTransport(AtomicUsize::new(0));
    let registry = ConnectorRegistry::new([descriptor("fixture")]).unwrap();
    assert_eq!(registry.list().len(), 1);
    assert_eq!(transport.0.load(Ordering::SeqCst), 0);
    futures::executor::block_on(async {
        assert_eq!(transport.identify().await.unwrap().0, 42);
        assert_eq!(
            transport
                .receive(ConnectorReceiveRequest {
                    cursor: "native".into()
                })
                .await
                .unwrap()
                .cursor,
            "native"
        );
        assert_eq!(
            transport
                .send(ConnectorSendRequest {
                    destination: "chat".into(),
                    text: "body".into(),
                    attempt_id: "attempt".into()
                })
                .await
                .unwrap()
                .attempt,
            "attempt"
        );
    });
    assert_eq!(transport.0.load(Ordering::SeqCst), 1);
    assert_eq!(FixtureFailure.kind(), ConnectorErrorKind::Uncertain);
}
