//! A fake accountd that serves only `org.quire.Spaces1` on a private bus: the list the test
//! sets, and the `Changed` signal it sends on demand.

use porter_dbus::{Details, SPACE_KEY_CREATED, SPACE_KEY_LOOK, SPACE_KEY_NAME, SPACES_PATH};
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

const ACCOUNTS: &str = "org.quire.Accounts1";

/// The Spaces accountd would list.
#[derive(Debug, Clone, Default)]
pub struct FakeSpaces {
    ids: Arc<Mutex<Vec<String>>>,
}

fn owned(value: Value<'_>) -> OwnedValue {
    OwnedValue::try_from(value).expect("owned value")
}

fn details(id: &str) -> Details {
    Details::from([
        (SPACE_KEY_NAME.to_owned(), owned(Value::from(id.to_owned()))),
        (SPACE_KEY_LOOK.to_owned(), owned(Value::from(String::new()))),
        (SPACE_KEY_CREATED.to_owned(), owned(Value::from(1_i64))),
    ])
}

struct Skeleton(FakeSpaces);

#[zbus::interface(name = "org.quire.Spaces1")]
impl Skeleton {
    fn list(&self) -> Vec<(String, Details)> {
        let ids = self.0.ids.lock().expect("lock").clone();
        ids.iter().map(|id| (id.clone(), details(id))).collect()
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>, id: &str, what: &str) -> zbus::Result<()>;
}

impl FakeSpaces {
    /// Serves `ids` on `connection` under accountd's name.
    pub async fn serve(connection: &zbus::Connection, ids: &[&str]) -> FakeSpaces {
        let spaces = FakeSpaces {
            ids: Arc::new(Mutex::new(ids.iter().map(|id| (*id).to_owned()).collect())),
        };
        connection
            .object_server()
            .at(SPACES_PATH, Skeleton(spaces.clone()))
            .await
            .expect("serve Spaces1");
        connection.request_name(ACCOUNTS).await.expect("name");
        spaces
    }

    /// The Space is removed: gone from the list, and everyone told.
    pub async fn remove(&self, connection: &zbus::Connection, id: &str) {
        self.ids.lock().expect("lock").retain(|held| held != id);
        let emitter = SignalEmitter::new(connection, SPACES_PATH).expect("emitter");
        Skeleton::changed(&emitter, id, "removed")
            .await
            .expect("signal");
    }
}
