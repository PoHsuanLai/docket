//! The helpers on the bus: `Companion1.Helpers` as it was, and `Companion1.Helpers2`, which adds
//! signing in to an app a helper runs and the routines. Only the shell speaks for the person on
//! either, except that our own editor bridge may read the list and a helper's card; every call
//! goes to the desks the daemon was built with. Members carry no doc comments:
//! zbus copies them into the introspection, which is held to `dbus/org.quire.Companion1.xml`.

use crate::routines::RoutineDesk;
use crate::serve::{bad, json};
use crate::speaker::{Call, EDITOR_APP, Speaker, Speakers};
use companion_wire::{HelperForm, ProposalId, RoutineForm, RoutineReply};
use docket_core::routine::RoutineId;
use docket_core::{SeenCommand, TurnId, TurnSource};
use docket_tasks::{HelperDesk, PostFromPerson, SayFromPerson};
use porter_core::AppName;
use prov::{EntityId, HelperId, SessionId};
use std::sync::Arc;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

/// The id of a helper as the shell sends it (the id's own text).
fn helper_of(text: &str) -> fdo::Result<HelperId> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(bad)
}

/// The id of a routine as the shell sends it (`r-3`).
fn routine_of(text: &str) -> fdo::Result<RoutineId> {
    RoutineId::parse(text).ok_or_else(|| fdo::Error::InvalidArgs("there is no such routine".into()))
}

/// The id of a proposed routine as the shell sends it (`p-3`).
fn proposal_of(text: &str) -> fdo::Result<ProposalId> {
    ProposalId::parse(text)
        .ok_or_else(|| fdo::Error::InvalidArgs("there is no such proposal".into()))
}

/// Where the router recorded a turn said by `speaker`: the editor bridge records as an editor,
/// the shell as the launcher. No other speaker speaks in a conversation.
fn turn_source(speaker: &Speaker) -> fdo::Result<TurnSource> {
    match speaker {
        Speaker::Shell => Ok(TurnSource::Launcher),
        Speaker::Editor => AppName::parse(EDITOR_APP)
            .map(TurnSource::Editor)
            .map_err(|_| fdo::Error::Failed("the editor bridge has no app name".into())),
        Speaker::Terminal(_) => Err(fdo::Error::AccessDenied(
            "a terminal does not speak in a helper's conversation".into(),
        )),
    }
}

/// What the helpers' calls come to, once, for both interfaces.
async fn post(
    desk: &dyn HelperDesk,
    helper: &str,
    goal: String,
    things: &str,
) -> fdo::Result<String> {
    let things: Vec<EntityId> = if things.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(things).map_err(bad)?
    };
    json(
        &desk
            .post_job(PostFromPerson {
                helper: helper_of(helper)?,
                goal,
                things,
            })
            .await,
    )
}

fn card(desk: &dyn HelperDesk, helper: &str) -> fdo::Result<String> {
    match desk.card(&helper_of(helper)?) {
        Some(card) => json(&card),
        None => Err(fdo::Error::InvalidArgs("there is no such helper".into())),
    }
}

async fn create(desk: &dyn HelperDesk, form: &str) -> fdo::Result<String> {
    let form: HelperForm = serde_json::from_str(form).map_err(bad)?;
    json(&desk.create(form).await)
}

async fn edit(desk: &dyn HelperDesk, helper: &str, form: &str) -> fdo::Result<String> {
    let form: HelperForm = serde_json::from_str(form).map_err(bad)?;
    json(&desk.edit(helper_of(helper)?, form).await)
}

/// The helpers the person keeps, on the same object as `Companion1`.
pub(crate) struct HelpersObject {
    pub(crate) desk: Arc<dyn HelperDesk>,
    pub(crate) speakers: Speakers,
}

impl HelpersObject {
    async fn require(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.speakers
            .require(header, Call::Helpers)
            .await
            .map(|_| ())
    }

    async fn require_read(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.speakers
            .require(header, Call::HelpersRead)
            .await
            .map(|_| ())
    }
}

#[zbus::interface(name = "org.quire.Companion1.Helpers")]
impl HelpersObject {
    async fn list(&self, after: u64, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        self.require_read(&header).await?;
        json(&self.desk.view(after))
    }

    async fn card(
        &self,
        helper: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require_read(&header).await?;
        card(self.desk.as_ref(), &helper)
    }

    async fn create(
        &self,
        form: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        create(self.desk.as_ref(), &form).await
    }

    async fn edit(
        &self,
        helper: String,
        form: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        edit(self.desk.as_ref(), &helper, &form).await
    }

    async fn pause(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.pause(helper_of(&helper)?).await;
        Ok(())
    }

    async fn resume(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.resume(helper_of(&helper)?).await;
        Ok(())
    }

    async fn remove(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.remove(helper_of(&helper)?).await;
        Ok(())
    }

    async fn post_job(
        &self,
        helper: String,
        goal: String,
        things: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        post(self.desk.as_ref(), &helper, goal, &things).await
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// The helpers and their routines, the second version of the interface.
pub(crate) struct Helpers2Object {
    pub(crate) desk: Arc<dyn HelperDesk>,
    pub(crate) routines: Option<Arc<dyn RoutineDesk>>,
    pub(crate) speakers: Speakers,
}

impl Helpers2Object {
    async fn require(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.speakers
            .require(header, Call::Helpers)
            .await
            .map(|_| ())
    }

    async fn require_read(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.speakers
            .require(header, Call::HelpersRead)
            .await
            .map(|_| ())
    }

    /// The routines, once the speaker may ask for them.
    async fn kept(&self, header: &Header<'_>) -> fdo::Result<&dyn RoutineDesk> {
        self.require(header).await?;
        self.routines
            .as_deref()
            .ok_or_else(|| fdo::Error::Failed("Routines are not kept here.".into()))
    }
}

#[zbus::interface(name = "org.quire.Companion1.Helpers2")]
impl Helpers2Object {
    async fn list(&self, after: u64, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        self.require_read(&header).await?;
        json(&self.desk.view(after))
    }

    async fn card(
        &self,
        helper: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require_read(&header).await?;
        card(self.desk.as_ref(), &helper)
    }

    async fn create(
        &self,
        form: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        create(self.desk.as_ref(), &form).await
    }

    async fn edit(
        &self,
        helper: String,
        form: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        edit(self.desk.as_ref(), &helper, &form).await
    }

    async fn pause(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.pause(helper_of(&helper)?).await;
        Ok(())
    }

    async fn resume(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.resume(helper_of(&helper)?).await;
        Ok(())
    }

    async fn remove(&self, helper: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.remove(helper_of(&helper)?).await;
        Ok(())
    }

    async fn post_job(
        &self,
        helper: String,
        goal: String,
        things: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        post(self.desk.as_ref(), &helper, goal, &things).await
    }

    async fn sign_in(
        &self,
        helper: String,
        way: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        json(&self.desk.sign_in(helper_of(&helper)?, way).await)
    }

    async fn sign_out(
        &self,
        helper: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        json(&self.desk.sign_out(helper_of(&helper)?).await)
    }

    async fn routines(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        json(&self.kept(&header).await?.view())
    }

    async fn routine_card(
        &self,
        routine: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let found = self.kept(&header).await?.card(routine_of(&routine)?);
        match found {
            Some(card) => json(&card),
            None => Err(fdo::Error::InvalidArgs("there is no such routine".into())),
        }
    }

    async fn add_routine(
        &self,
        form: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let routines = self.kept(&header).await?;
        let form: RoutineForm = serde_json::from_str(&form).map_err(bad)?;
        json(&routines.create(form).await)
    }

    async fn pause_routine(
        &self,
        routine: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.kept(&header).await?.pause(routine_of(&routine)?).await;
        Ok(())
    }

    async fn resume_routine(
        &self,
        routine: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.kept(&header)
            .await?
            .resume(routine_of(&routine)?)
            .await;
        Ok(())
    }

    async fn remove_routine(
        &self,
        routine: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.kept(&header)
            .await?
            .remove(routine_of(&routine)?)
            .await;
        Ok(())
    }

    async fn routine_proposals(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        self.require_read(&header).await?;
        json(&self.desk.routine_proposals())
    }

    async fn accept_routine_proposal(
        &self,
        id: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let routines = self.kept(&header).await?;
        let found = self.desk.take_routine_proposal(proposal_of(&id)?);
        match found {
            Some(proposal) => json(&routines.create(proposal.form).await),
            None => json(&RoutineReply::Refused(
                "That suggestion is no longer there.".to_owned(),
            )),
        }
    }

    async fn decline_routine_proposal(
        &self,
        id: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.require(&header).await?;
        self.desk.take_routine_proposal(proposal_of(&id)?);
        Ok(())
    }

    async fn open_in_terminal(
        &self,
        helper: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        json(&self.desk.open_in_terminal(helper_of(&helper)?).await)
    }

    async fn bring_back(
        &self,
        helper: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.require(&header).await?;
        json(&self.desk.bring_back(helper_of(&helper)?).await)
    }

    async fn say(
        &self,
        session: String,
        turn: u64,
        text: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let speaker = self.speakers.require(&header, Call::Converse).await?;
        let session = SessionId::parse(&session)
            .map_err(|_| fdo::Error::InvalidArgs("there is no such conversation".into()))?;
        json(
            &self
                .desk
                .say(SayFromPerson {
                    session,
                    id: TurnId(turn),
                    text,
                    from: turn_source(&speaker)?,
                })
                .await,
        )
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// The conversations a terminal started with an agent app, which the person can bring in. Only the
/// shell speaks for the person here.
pub(crate) struct TerminalsObject {
    pub(crate) desk: Arc<dyn HelperDesk>,
    pub(crate) speakers: Speakers,
}

#[zbus::interface(name = "org.quire.Companion1.Terminals")]
impl TerminalsObject {
    async fn sessions(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        self.speakers.require(&header, Call::Helpers).await?;
        json(&self.desk.terminal_sessions().await)
    }

    async fn bring_back(
        &self,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.speakers.require(&header, Call::Helpers).await?;
        let session = SessionId::parse(&session)
            .map_err(|_| fdo::Error::InvalidArgs("there is no such conversation".into()))?;
        json(&self.desk.bring_back_terminal(session).await)
    }

    async fn recognise(
        &self,
        seen: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        self.speakers.require(&header, Call::Recognise).await?;
        let seen: SeenCommand = serde_json::from_str(&seen)
            .map_err(|_| fdo::Error::InvalidArgs("that is not a command line".into()))?;
        json(&self.desk.recognise(seen).await)
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
