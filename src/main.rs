use dialoguer::{theme::ColorfulTheme, Select};
use git2::{BranchType, Repository, RepositoryState};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::str;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "redmine-new-branch",
    about = "Create a new git branch following your team naming."
)]
struct Opt {
    /// Activate debug mode ( not implemented )
    // short and long flags (-v, --verbose) will be deduced from the field's name
    #[structopt(short, long)]
    verbose: bool,

    // Don't create git branch
    #[structopt(short, long)]
    dry_run: bool,

    /// Set redmine ticket
    #[structopt(short, long)]
    ticket: i64,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct MyConfig {
    version: u8,
    api_key: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct IdProperty {
    id: i32,
}
#[derive(Serialize, Deserialize, Debug)]
struct NamedProperty {
    id: i32,
    name: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct NamedPropertyWithOptionValue {
    id: i32,
    name: String,
    value: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Issue {
    id: i32,
    subject: String,
    fixed_version: NamedProperty,
    assigned_to: NamedProperty,
    custom_fields: Vec<NamedPropertyWithOptionValue>,
    parent: Option<IdProperty>,
}

impl Issue {
    fn target_version(&self) -> &str {
        &self.fixed_version.name[..3]
    }

    fn cleanup_subject(subject: &String) -> String {
        let mut subject = subject
            .trim()
            .replace(" ", "-")
            .replace(":", "=")
            .to_lowercase();

        // Replace multiple -- by only one -
        let re_multiple_dash = Regex::new(r"-+").unwrap();
        let re_forbidden_char = Regex::new(r#"[\[\]"'\)\()]*"#).unwrap();

        subject = re_multiple_dash.replace_all(&subject, "-").to_string();
        subject = re_forbidden_char.replace_all(&subject, "").to_string();
        // Replace "à" by "a", "é" by "e" ...
        subject = diacritics::remove_diacritics(&subject);

        subject
    }

    fn get_branch_name(&self) -> String {
        let v: Vec<&str> = self.assigned_to.name.split(' ').collect();

        if v.len() < 2 {
            panic!("Unable to read trigram")
        }

        let subject = Issue::cleanup_subject(&self.subject);

        format!(
            "rd-{number}-{trigram}-{version}-{subject}",
            number = self.id,
            subject = subject,
            version = &self.target_version(),
            trigram = format!("{}{}", &v[0][..1], &v[1][..2]).to_lowercase()
        )
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Ticket {
    issue: Issue,
}

fn read_issue(body: &str) -> serde_json::Result<Ticket> {
    serde_json::from_str(&body)
}

fn get_ticket_body(ticket: i64, key: String) -> Result<String, reqwest::Error> {
    let ticket_url = format!(
        "https://redmine.corp.wallix.com/issues/{ticket}.json",
        ticket = ticket
    );

    println!("Requesting {:?}...", ticket_url);

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;

    client
        .get(&ticket_url)
        .header("X-Redmine-API-Key", key)
        .send()?
        .text()
}

fn create_new_branch(ticket: Ticket) -> Result<(), git2::Error> {
    let path = env::current_dir().unwrap();
    let repo = Repository::discover(path)?;

    println!("Repo found at : {}", repo.path().to_string_lossy());

    if repo.state() == RepositoryState::Clean {
        println!("Repo is clean");
    }

    let work = repo.diff_index_to_workdir(None, None)?;

    println!(
        "Number of files changed in workdir = {:?}",
        work.stats()?.files_changed()
    );

    let remotes = repo.remotes()?;

    if remotes.len() != 1 {
        panic!("I don't know what to do with more than one git remote repository")
    }

    let remote_name = remotes.get(0).unwrap_or("origin");

    let mut source_branch = format!("{}/{}", remote_name, "master");

    let head = repo.head()?;
    let head_ref = head.name().unwrap();

    let remote_branches: Vec<String> = repo
        .branches(Some(BranchType::Remote))?
        .into_iter()
        .filter_map(|b| {
            if let Ok((branch, _)) = b {
                if let Ok(Some(branch_name)) = branch.name() {
                    return Some(branch_name.to_string());
                }
            }
            return None;
        })
        .collect();

    // println!(
    //     "List of all {} remote branchs {:?}",
    //     remote_branchs.len(),
    //     remote_branchs
    // );

    if head_ref.ends_with(&ticket.issue.get_branch_name()) {
        println!(
            "We are already in the desired branch {}",
            ticket.issue.get_branch_name()
        );
        return Ok(());
    }

    // Check if target branch already exists !

    let branch_containing_this_ticket = remote_branches
        .clone()
        .into_iter()
        .find(|name| name.contains(&ticket.issue.id.to_string()));

    if let Some(existing_branch) = branch_containing_this_ticket {
        println!(
            "I could create branch {} but the branch {} already exists for the ticket #{}",
            ticket.issue.get_branch_name(),
            existing_branch,
            ticket.issue.id,
        );
        return Ok(());
    }

    println!("Target version : {}", ticket.issue.target_version());

    let maintenance_branch_name = format!("{}/wab-{}", remote_name, ticket.issue.target_version());

    // Search if there is a maintenance branch for this version
    let is_maintenance_branch_existing: bool = !remote_branches
        .clone()
        .into_iter()
        .find(|b| maintenance_branch_name.eq(b))
        .is_none();

    if is_maintenance_branch_existing {
        source_branch = maintenance_branch_name;
    } else {
        if let Some(p) = &ticket.issue.parent {
            let sources: Vec<String> = remote_branches
                .into_iter()
                .filter(|name| name.contains(&p.id.to_string()))
                .collect();

            if sources.len() > 0 {
                let selections: &[&str] = &[&source_branch, &sources[0]];

                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("This ticket has a parent, what branch use to be based on ?")
                    .default(0)
                    .items(&selections[..])
                    .interact()
                    .unwrap();

                source_branch = selections[selection].to_string();
            } else {
                println!(
                    "This ticket has {} as parent but the branch don't exist",
                    &p.id
                )
            }
        } else {
            println!("This ticket has no parent")
        }
    }

    for b in repo.branches(Some(BranchType::Remote))? {
        let (b, _) = b?;
        let name = b.name()?.unwrap();

        if name == source_branch {
            println!("I found {} !", name);
            let reference = b.get();
            let name_new_branch = ticket.issue.get_branch_name();
            println!(
                "Let's create branch {} based on {}",
                name_new_branch, source_branch
            );
            let commit = reference.peel_to_commit()?;
            // create the new branch based on this commit
            repo.branch(&name_new_branch, &commit, false).unwrap();
            //checkout to this branch
            let obj = repo
                .revparse_single(&format!("refs/heads/{}", name_new_branch))
                .unwrap();
            repo.checkout_tree(&obj, None)?;
            return repo.set_head(&format!("refs/heads/{}", name_new_branch));
        }
    }

    Ok(())
}

fn main()-> Result<(), confy::ConfyError> {

    let app_name = env!("CARGO_PKG_NAME");
    let opt = Opt::from_args();

    let cfg: MyConfig = confy::load(app_name, None)?;

    let config_path = confy::get_configuration_file_path(app_name, None)?;

    if opt.verbose {
        println!("Reading config in {:?}",config_path);
    }

    if cfg.api_key == "" {
        println!("No api key found, storing default config file in {:?}...",config_path);
        confy::store(app_name, None, &cfg)?;
    }

    let body = get_ticket_body(opt.ticket, cfg.api_key);

    let body = match body {
        Ok(body) => body,
        Err(e) => panic!("Unable to fetch Redmine API : {}", e),
    };

    let ticket = read_issue(&body);

    let ticket = match ticket {
        Ok(t) => t,
        Err(e) => panic!("Unable to decode json \"{}\" => {}", body, e),
    };

    if !opt.dry_run {
        match create_new_branch(ticket) {
            Ok(()) => {}
            Err(e) => println!("Error : {}", e),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use serde_json;

    #[test]
    fn issue_parsing() -> Result<(), serde_json::Error> {
        let example = r#"
        {
            "issue":{
                "id":26968,
                "project":{"id":27,"name":"Bastion UI"},
                "tracker":{"id":1,"name":"Bug"},
                "status":{"id":3,"name":"Resolved"},
                "priority":{"id":4,"name":"Normal"},
                "author":{"id":87,"name":"toto"},
                "assigned_to":{"id":220,"name":"tata"},
                "category":{"id":328,"name":"_Targets"},
                "fixed_version":{"id":318,"name":"8.1.0"},
                "subject":"The duration fields in checkout policy must include seconds",
                "description":"description",
                "start_date":"2020-04-17","done_ratio":0,"spent_hours":0.0,"total_spent_hours":0.0,
                "custom_fields":[
                    {"id":2,"name":"Severity","value":"Medium"},
                    {"id":6,"name":"Affected Bastion version","value":"8.0.1"},
                    {"id":7,"name":"Weight","value":""},
                    {"id":16,"name":"Fixed in tag","value":"8.1.0.0"},
                    {"id":20,"name":"Affected build","value":""},
                    {"id":34,"name":"Difficulty","value":"4"},
                    {"id":21,"name":"Regression","value":"0"},
                    {"id":30,"name":"Doc changes","value":"2"},
                    {"id":31,"name":"Sprint number","value":"64"},
                    {"id":33,"name":"CVE","value":"0"},
                    {"id":39,"name":"CVE List","value":""},
                    {"id":50,"name":"Developer","value":"220"}],
                "created_on":"2020-04-17T08:34:16Z",
                "updated_on":"2020-05-07T15:40:30Z"
            }
        }
        "#;

        let result = read_issue(&example.to_string())?;

        assert_eq!(result.issue.id, 26968);

        Ok(())
    }

    #[test]
    fn test_branch_name() {
        let t = Ticket {
            issue: Issue {
                id: 42,
                subject: String::from("[Do] stuff \"asap\" "),
                assigned_to: NamedProperty {
                    id: 220,
                    name: String::from("Arnold Bcon Tran"),
                },
                fixed_version: NamedProperty {
                    id: 318,
                    name: String::from("8.1.0"),
                },
                custom_fields: vec![
                    NamedPropertyWithOptionValue {
                        id: 50,
                        name: String::from("Developer"),
                        value: Some(String::from("220")),
                    },
                    NamedPropertyWithOptionValue {
                        id: 50,
                        name: String::from("SF Case"),
                        value: None,
                    },
                ],
                parent: None,
            },
        };
        assert_eq!(t.issue.get_branch_name(), "rd-42-abc-8.1-do-stuff-asap");
    }

    #[test]
    fn test_subject_cleanup() {
        assert_eq!(Issue::cleanup_subject(&String::from("-----")), "-");
        assert_eq!(Issue::cleanup_subject(&String::from("  - -  - -  ")), "-");
        assert_eq!(Issue::cleanup_subject(&String::from("it's a clean()")), "its-a-clean");
        assert_eq!(
            Issue::cleanup_subject(&String::from(" [Do] the - \"laundry\" ")),
            "do-the-laundry"
        );
    }
}
