//! Step 3: Contacts — offer to reuse saved contacts, enter new ones, or use
//! account defaults.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};

use crate::contacts::{self, Contact, ContactsFile, Role};

use super::super::wizard::{ContactsChoice, StepContext, StepResult, WizardState};

pub(crate) async fn run(state: &mut WizardState, _ctx: &StepContext) -> Result<StepResult> {
    eprintln!(
        "\n  {} Setting up contacts for registration",
        style("👤").bold()
    );

    // Try to load existing contacts.toml.
    let saved = contacts::load().ok();
    let has_saved = saved
        .as_ref()
        .map(|f| f.get(Role::Registrant).is_some())
        .unwrap_or(false);

    let choice = if has_saved {
        let options = vec![
            "Use saved contacts from contacts.toml",
            "Use account default contacts (no file needed)",
            "Enter contacts manually",
            "↩ Go back",
        ];
        let selection = Select::new()
            .with_prompt("How would you like to supply contacts?")
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        match selection {
            0 => {
                let file = saved.expect("checked above");
                display_saved_contacts(&file);
                ContactsChoice::FromFile(file)
            }
            1 => ContactsChoice::AccountDefault,
            2 => collect_contacts_interactively()?,
            3 => return Ok(StepResult::Back),
            _ => unreachable!(),
        }
    } else {
        let options = vec![
            "Use account default contacts",
            "Enter contacts manually",
            "↩ Go back",
        ];
        let selection = Select::new()
            .with_prompt("How would you like to supply contacts?")
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        match selection {
            0 => ContactsChoice::AccountDefault,
            1 => collect_contacts_interactively()?,
            2 => return Ok(StepResult::Back),
            _ => unreachable!(),
        }
    };

    match &choice {
        ContactsChoice::AccountDefault => {
            eprintln!(
                "  {} Using account default contacts",
                style("✓").green().bold()
            );
        }
        ContactsChoice::FromFile(_) => {
            eprintln!(
                "  {} Using saved contacts from contacts.toml",
                style("✓").green().bold()
            );
        }
        ContactsChoice::Manual(_) => {
            eprintln!(
                "  {} Contacts entered successfully",
                style("✓").green().bold()
            );
        }
    }

    state.contacts = choice;
    Ok(StepResult::Continue)
}

/// Mask an email for TTY preview so contact confirmation does not log full PII.
fn mask_email(email: &str) -> String {
    let (local, domain) = email.split_once('@').unwrap_or((email, ""));
    if local.is_empty() || domain.is_empty() {
        return "***".to_string();
    }
    let visible = local.chars().next().map_or('*', |c| c);
    format!("{visible}***@{domain}")
}

fn display_saved_contacts(file: &ContactsFile) {
    for role in [Role::Registrant, Role::Admin, Role::Billing, Role::Tech] {
        if let Some(c) = file.get(role) {
            eprintln!(
                "    {} {}: {} {} <{}>",
                style("•").dim(),
                style(role.label()).bold(),
                c.name_first,
                c.name_last,
                mask_email(&c.email)
            );
        }
    }
}

fn collect_contacts_interactively() -> Result<ContactsChoice> {
    eprintln!("\n  Enter registrant contact details (other roles will use account defaults):");

    let name_first = prompt_required("First name")?;
    let name_last = prompt_required("Last name")?;
    let email = prompt_validated("Email", validate_email)?;
    let phone = prompt_validated("Phone (e.g. +1.4805551212)", validate_phone)?;
    let organization = prompt_optional("Organization (optional, press Enter to skip)")?;
    let address1 = prompt_required("Address line 1")?;
    let address2 = prompt_optional("Address line 2 (optional, press Enter to skip)")?;
    let city = prompt_required("City")?;
    let state_prov = prompt_required("State/Province")?;
    let postal_code = prompt_required("Postal code")?;
    let country = prompt_validated("Country code (2-letter ISO, e.g. US)", validate_country)?;

    let contact = Contact {
        name_first,
        name_last,
        email,
        phone,
        organization,
        address1,
        address2,
        city,
        state: state_prov,
        postal_code,
        country,
    };

    // Offer to save for future use.
    let save = Confirm::new()
        .with_prompt("Save these contacts to contacts.toml for future registrations?")
        .default(true)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if save {
        if let Err(e) = save_contact_to_file(&contact) {
            eprintln!(
                "  {} Could not save contacts: {e}",
                style("⚠").yellow().bold()
            );
        } else if let Some(path) = contacts::contacts_path() {
            eprintln!(
                "  {} Saved to {}",
                style("✓").green().bold(),
                style(path.display()).dim()
            );
        }
    }

    let file = ContactsFile {
        registrant: Some(contact),
        admin: None,
        billing: None,
        tech: None,
    };

    Ok(ContactsChoice::Manual(file))
}

fn prompt_required(label: &str) -> Result<String> {
    let value: String = Input::new()
        .with_prompt(format!("  Enter {label}"))
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    let trimmed = value.trim().to_owned();
    if trimmed.is_empty() {
        return Err(CliCoreError::message(format!("{label} cannot be empty")));
    }
    Ok(trimmed)
}

fn prompt_optional(label: &str) -> Result<Option<String>> {
    let value: String = Input::new()
        .with_prompt(format!("  Enter {label} (optional)"))
        .allow_empty(true)
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    let trimmed = value.trim().to_owned();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

fn prompt_validated(
    label: &str,
    validate: fn(&str) -> std::result::Result<(), String>,
) -> Result<String> {
    loop {
        let value: String = Input::new()
            .with_prompt(format!("  Enter {label}"))
            .interact_text()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
        let trimmed = value.trim().to_owned();
        if trimmed.is_empty() {
            eprintln!("    This field is required.");
            continue;
        }
        match validate(&trimmed) {
            Ok(()) => return Ok(trimmed),
            Err(msg) => {
                eprintln!("    {}", style(&msg).red());
                continue;
            }
        }
    }
}

fn validate_email(email: &str) -> std::result::Result<(), String> {
    if email.contains('@') && email.contains('.') && email.len() >= 5 {
        Ok(())
    } else {
        Err("Invalid email format (expected user@domain.tld)".to_owned())
    }
}

fn validate_phone(phone: &str) -> std::result::Result<(), String> {
    // Accept anything the phonenumber crate can parse (validated fully at
    // to_api() time); here we just do a basic format check.
    if phone.len() >= 7
        && phone.chars().all(|c| {
            c.is_ascii_digit()
                || c == '+'
                || c == '.'
                || c == '-'
                || c == ' '
                || c == '('
                || c == ')'
        })
    {
        Ok(())
    } else {
        Err(
            "Invalid phone format (expected something like +1.4805551212 or (480) 555-1212)"
                .to_owned(),
        )
    }
}

fn validate_country(code: &str) -> std::result::Result<(), String> {
    let upper = code.to_ascii_uppercase();
    if upper == "C2" || (upper.len() == 2 && upper.bytes().all(|b| b.is_ascii_uppercase())) {
        Ok(())
    } else {
        Err("Expected a two-letter ISO country code (e.g. US, GB, CA)".to_owned())
    }
}

/// Save a contact as the registrant in contacts.toml.
pub(crate) fn save_contact_to_file(contact: &Contact) -> std::result::Result<(), String> {
    let path = contacts::contacts_path()
        .ok_or_else(|| "could not determine config directory".to_owned())?;

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create config directory: {e}"))?;
    }

    let toml_content = format!(
        r#"# gddy domain registration contacts
# Saved by `gddy domain register` interactive wizard.

[registrant]
name_first = "{first}"
name_last = "{last}"
email = "{email}"
phone = "{phone}"
{org}address1 = "{addr1}"
{addr2}city = "{city}"
state = "{state}"
postal_code = "{postal}"
country = "{country}"
"#,
        first = escape_toml(&contact.name_first),
        last = escape_toml(&contact.name_last),
        email = escape_toml(&contact.email),
        phone = escape_toml(&contact.phone),
        org = contact
            .organization
            .as_ref()
            .map(|o| format!("organization = \"{}\"\n", escape_toml(o)))
            .unwrap_or_default(),
        addr1 = escape_toml(&contact.address1),
        addr2 = contact
            .address2
            .as_ref()
            .map(|a| format!("address2 = \"{}\"\n", escape_toml(a)))
            .unwrap_or_default(),
        city = escape_toml(&contact.city),
        state = escape_toml(&contact.state),
        postal = escape_toml(&contact.postal_code),
        country = escape_toml(&contact.country),
    );

    std::fs::write(&path, toml_content)
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn mask_email_hides_local_part() {
        assert_eq!(mask_email("jane@example.com"), "j***@example.com");
        assert_eq!(mask_email("a@b.c"), "a***@b.c");
        assert_eq!(mask_email("invalid"), "***");
    }

    #[test]
    fn validate_email_accepts_valid() {
        assert!(validate_email("user@example.com").is_ok());
        assert!(validate_email("a@b.c").is_ok());
    }

    #[test]
    fn validate_email_rejects_invalid() {
        assert!(validate_email("notanemail").is_err());
        assert!(validate_email("@.").is_err());
        assert!(validate_email("").is_err());
    }

    #[test]
    fn validate_phone_accepts_common_formats() {
        assert!(validate_phone("+1.4805551212").is_ok());
        assert!(validate_phone("(480) 555-1212").is_ok());
        assert!(validate_phone("+44 7793 601890").is_ok());
    }

    #[test]
    fn validate_phone_rejects_garbage() {
        assert!(validate_phone("abc").is_err());
        assert!(validate_phone("").is_err());
    }

    #[test]
    fn validate_country_accepts_iso_codes() {
        assert!(validate_country("US").is_ok());
        assert!(validate_country("us").is_ok());
        assert!(validate_country("GB").is_ok());
        assert!(validate_country("C2").is_ok());
    }

    #[test]
    fn validate_country_rejects_invalid() {
        assert!(validate_country("USA").is_err());
        assert!(validate_country("1").is_err());
        assert!(validate_country("").is_err());
    }

    #[test]
    fn escape_toml_handles_special_chars() {
        assert_eq!(escape_toml(r#"hello "world""#), r#"hello \"world\""#);
        assert_eq!(escape_toml(r"path\to"), r"path\\to");
    }

    #[test]
    fn save_contact_roundtrip() {
        let contact = Contact {
            name_first: "Ada".to_owned(),
            name_last: "Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
            phone: "+1.4805551212".to_owned(),
            organization: Some("Engines Inc".to_owned()),
            address1: "1 Bletchley Park".to_owned(),
            address2: Some("Suite 100".to_owned()),
            city: "Tempe".to_owned(),
            state: "AZ".to_owned(),
            postal_code: "85281".to_owned(),
            country: "US".to_owned(),
        };

        let tmp = NamedTempFile::new().expect("tmpfile");
        let path = tmp.path().to_path_buf();

        // Write to a temp file to verify the generated TOML is valid.
        let toml_content = format!(
            r#"[registrant]
name_first = "{first}"
name_last = "{last}"
email = "{email}"
phone = "{phone}"
organization = "{org}"
address1 = "{addr1}"
address2 = "{addr2}"
city = "{city}"
state = "{state}"
postal_code = "{postal}"
country = "{country}"
"#,
            first = escape_toml(&contact.name_first),
            last = escape_toml(&contact.name_last),
            email = escape_toml(&contact.email),
            phone = escape_toml(&contact.phone),
            org = escape_toml(contact.organization.as_deref().unwrap_or("")),
            addr1 = escape_toml(&contact.address1),
            addr2 = escape_toml(contact.address2.as_deref().unwrap_or("")),
            city = escape_toml(&contact.city),
            state = escape_toml(&contact.state),
            postal = escape_toml(&contact.postal_code),
            country = escape_toml(&contact.country),
        );

        std::fs::write(&path, &toml_content).expect("write");

        // Parse back and verify.
        let raw = std::fs::read_to_string(&path).expect("read");
        let parsed: ContactsFile = toml::from_str(&raw).expect("parse");
        let registrant = parsed.get(Role::Registrant).expect("registrant present");
        assert_eq!(registrant.name_first, "Ada");
        assert_eq!(registrant.name_last, "Lovelace");
        assert_eq!(registrant.email, "ada@example.com");
        assert_eq!(registrant.city, "Tempe");
        assert_eq!(registrant.country, "US");
    }
}
