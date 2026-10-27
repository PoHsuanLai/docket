//! The manifests the fakes ship with, and `fake_router`.

use crate::clock::FixedClock;
use crate::files::FakeFiles;
use crate::mail::FakeMail;
use crate::menu::FakeMenu;
use crate::scripted::{
    FakeMemory, ScriptedConfirmer, ScriptedReader, ScriptedReviewer, ScriptedWriter,
};
use crate::seams::{FakeLink, FakeSeams};
use crate::simple::{MemoryGrants, RecordingSink};
use action_review::Reviewer;
use docket_core::Clock;
use docket_core::{AgentConfig, PolicyWriter, ValidManifest};
use docket_core::{ManifestFileError, parse};
use docket_router::{Registry, Router};
use docket_session::SessionLog;
use docket_session::fake::MemoryLog;
use policy_point::{PolicyError, Presets};
use prov::{SpaceId, UnixSeconds};
use std::sync::Arc;

/// The fixture manifest of the fake mail app.
pub const MAIL_MANIFEST: &str = include_str!("../fixtures/manifests/org.quire.Mail.intents.toml");
/// The fixture manifest of the fake files app.
pub const FILES_MANIFEST: &str = include_str!("../fixtures/manifests/org.quire.Files.intents.toml");
/// The fixture manifest of the fake menu app (`menu.item.activate` classifies per call).
pub const MENU_MANIFEST: &str = include_str!("../fixtures/manifests/org.quire.Menu.intents.toml");
const MEMORY_MANIFEST: &str = include_str!("../../../manifests/org.quire.Memory.toml");
const COMPANION_MANIFEST: &str = include_str!("../../../manifests/org.quire.Companion.toml");
const ACP_AGENT_MANIFEST: &str = include_str!("../../../manifests/org.quire.AcpAgent.toml");

/// Why a fake router could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FakeError {
    /// A shipped manifest does not parse or validate.
    #[error("manifest: {0}")]
    Manifest(ManifestFileError),
    /// The shipped policy set does not load.
    #[error("policy: {0}")]
    Policy(PolicyError),
    /// The fixture Space id is malformed.
    #[error("space")]
    Space,
}

/// The fixture mail manifest, validated.
pub fn mail_manifest() -> Result<ValidManifest, ManifestFileError> {
    parse(MAIL_MANIFEST)
}

/// The fixture files manifest, validated.
pub fn files_manifest() -> Result<ValidManifest, ManifestFileError> {
    parse(FILES_MANIFEST)
}

/// The fixture menu manifest, validated.
pub fn menu_manifest() -> Result<ValidManifest, ManifestFileError> {
    parse(MENU_MANIFEST)
}

/// A registry holding the two fixtures and the three shipped built-in manifests.
pub fn registry() -> Result<Registry, ManifestFileError> {
    let mut registry = Registry::new();
    for text in [
        MAIL_MANIFEST,
        FILES_MANIFEST,
        MEMORY_MANIFEST,
        COMPANION_MANIFEST,
        ACP_AGENT_MANIFEST,
    ] {
        registry.insert(parse(text)?);
    }
    Ok(registry)
}

/// Installs the fake menu app's manifest in `router` (it is not in `registry()`: the menu is only
/// for the tests of per-call effects, and the default world's goldens do not list it).
pub fn install_menu(router: &Router<FakeSeams>) -> Result<(), ManifestFileError> {
    let menu = menu_manifest()?;
    match router.state.lock() {
        Ok(mut state) => state.registry.insert(menu),
        Err(poisoned) => poisoned.into_inner().registry.insert(menu),
    };
    Ok(())
}

/// A router over the fakes, in the Space `work`, with the fixture and shipped manifests
/// installed, a reviewer that allows everything (a hijacked judge), no confirmer answers, a
/// writer that fails (no task policy), and the clock at the epoch. Tests replace what they
/// need through `router.seams`.
pub fn fake_router(config: AgentConfig) -> Result<Router<FakeSeams>, FakeError> {
    fake_router_with(
        config,
        ScriptedReviewer::always_allow(),
        ScriptedWriter::failing(),
        FixedClock::at(UnixSeconds(0)),
    )
}

/// [`fake_router`] with this reviewer, writer and clock in place of the fakes: what a live run
/// builds, with the real cascade and the real writer over inferd.
pub fn fake_router_with<R: Reviewer, W: PolicyWriter, K: Clock>(
    config: AgentConfig,
    reviewer: R,
    writer: W,
    clock: K,
) -> Result<Router<FakeSeams<R, W, K>>, FakeError> {
    fake_router_on(config, reviewer, writer, clock, Arc::new(MemoryLog::new()))
}

/// [`fake_router_with`] over `log`: the router a test builds after dropping the first one, to
/// restore sessions from what the first wrote.
pub fn fake_router_on<R: Reviewer, W: PolicyWriter, K: Clock, L: SessionLog>(
    config: AgentConfig,
    reviewer: R,
    writer: W,
    clock: K,
    log: L,
) -> Result<Router<FakeSeams<R, W, K, L>>, FakeError> {
    fake_router_over(
        config,
        ScriptedConfirmer::default(),
        reviewer,
        writer,
        clock,
        log,
    )
}

/// The router [`fake_router_over`] builds.
pub type Overed<R, W, K, L, C> = Router<FakeSeams<R, W, K, L, C>>;

/// A fake router whose reader is a real one ([`fake_router_reading`]).
pub type Reading<R, W, K, L, C, Rd> = Router<FakeSeams<R, W, K, L, C, Rd>>;

/// [`fake_router_on`] with this confirmer in place of the scripted one: a test whose sheets are
/// answered by something else (a desk the host of an external agent shows).
pub fn fake_router_over<
    R: Reviewer,
    W: PolicyWriter,
    K: Clock,
    L: SessionLog,
    C: docket_core::Confirmer,
>(
    config: AgentConfig,
    confirmer: C,
    reviewer: R,
    writer: W,
    clock: K,
    log: L,
) -> Result<Overed<R, W, K, L, C>, FakeError> {
    fake_router_reading(
        config,
        confirmer,
        reviewer,
        writer,
        clock,
        log,
        ScriptedReader::default(),
    )
}

/// [`fake_router_over`] with this reader in place of the scripted one: a live run whose planner
/// reads untrusted text through a real reader model.
pub fn fake_router_reading<
    R: Reviewer,
    W: PolicyWriter,
    K: Clock,
    L: SessionLog,
    C: docket_core::Confirmer,
    Rd: docket_core::Reader,
>(
    config: AgentConfig,
    confirmer: C,
    reviewer: R,
    writer: W,
    clock: K,
    log: L,
    reader: Rd,
) -> Result<Reading<R, W, K, L, C, Rd>, FakeError> {
    let space = SpaceId::parse("work").map_err(|_| FakeError::Space)?;
    let mail = mail_manifest().map_err(FakeError::Manifest)?;
    let files = files_manifest().map_err(FakeError::Manifest)?;
    let menu = menu_manifest().map_err(FakeError::Manifest)?;
    let seams = FakeSeams {
        link: FakeLink::new(
            FakeMail::new(mail, space.clone()),
            FakeFiles::new(files, space),
            FakeMenu::new(menu),
        ),
        confirmer,
        reviewer,
        grants: MemoryGrants::new(),
        sink: RecordingSink::new(),
        clock,
        memory: FakeMemory::default(),
        writer,
        reader,
        log,
        checkpoints: docket_checkpoint::MemoryStore::new(),
    };
    // The built-in core and both presets: the fake reads no policy files, so tests are the same everywhere.
    let presets = Presets::built_in().map_err(FakeError::Policy)?;
    let router = Router::new(seams, config, presets);
    let installed = registry().map_err(FakeError::Manifest)?;
    match router.state.lock() {
        Ok(mut state) => state.registry = installed,
        Err(poisoned) => poisoned.into_inner().registry = installed,
    }
    Ok(router)
}
