//! A mail app and sill's sheet, for trying `quire-do` by hand on a private bus
//! (`scripts/try-quire-do.sh` runs it). The mail app is docket-fake's `FakeMail` behind the real
//! `docket_client::serve_on`; the sheet is `Confirm1` asking on this terminal. Nothing here is
//! the person's real session: it serves whatever bus `DBUS_SESSION_BUS_ADDRESS` names.

#[path = "../tests/it/support/apps.rs"]
mod apps;

use apps::{Answer, FakeSill, serve_mail};
use docket_core::{ConfirmAnswer, ConfirmEnd, ConfirmOffer, ConfirmRequest, GrantScope};
use prov::{ConfirmId, ConfirmReceipt, InputProof, UnixSeconds};
use std::io::{BufRead, Write};

fn receipt() -> ConfirmReceipt {
    ConfirmReceipt {
        id: ConfirmId::parse("c-1").expect("id"),
        input: InputProof::SheetFallback,
        at: UnixSeconds(0),
        covers: prov::Confidentiality::Secret,
    }
}

/// Shows the sheet on this terminal and reads the person's answer.
fn ask(request: &ConfirmRequest) -> Answer {
    println!();
    println!(
        "--- sheet: {:?} wants to \"{}\" ({:?}, {} thing(s)); why: {:?}",
        request.actor, request.action, request.effect, request.count.0, request.why
    );
    let terminal = request.offer == ConfirmOffer::OnceOrFromTerminal;
    print!(
        "    [y] allow once{}  [anything else] refuse: ",
        if terminal {
            "   [t] allow from the terminal until logout"
        } else {
            ""
        }
    );
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    match line.trim() {
        "y" => Answer::With(ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt: receipt(),
        }),
        "t" if terminal => Answer::With(ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }),
        _ => Answer::With(ConfirmAnswer::Ended(ConfirmEnd::Refused)),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mail_connection = zbus::Connection::session().await?;
    let mail = serve_mail(&mail_connection).await;
    let sill_connection = zbus::Connection::session().await?;
    let _sill = FakeSill::start_asking(
        &sill_connection,
        &["org.quire.Confirm1", "org.quire.Shell"],
        ask,
    )
    .await;
    println!("mail (org.quire.Mail) and the sheet (org.quire.Confirm1, org.quire.Shell) are up.");
    println!("threads: t1 \"Invoice\", t2 \"Digest\". Ctrl-C to stop.");
    let _ = &mail;
    std::future::pending::<()>().await;
    Ok(())
}
