use git2::{BranchType, Repository, RepositoryState};
use serde::{Deserialize, Serialize};
use std::env;
use std::str;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "git-new-branch",
    about = "Create a new branch following your team naming."
)]
struct Opt {
    /// Activate debug mode
    // short and long flags (-d, --debug) will be deduced from the field's name
    #[structopt(short, long)]
    debug: bool,

    /// Set redmine ticket
    #[structopt(short, long)]
    ticket: i64,

    /// Redmine access key
    #[structopt(short, long)]
    key_redmine_api: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct NamedProperty {
    id: i32,
    name: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct NamedPropertyWithValue {
    id: i32,
    name: String,
    value: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Issue {
    id: i32,
    subject: String,
    fixed_version: NamedProperty,
    assigned_to: NamedProperty,
    custom_fields: Vec<NamedPropertyWithValue>,
}

impl Issue {
    fn target_version(&self) -> &str {
        &self.fixed_version.name[..3]
    }
    fn get_branch_name(&self) -> String {
        let v: Vec<&str> = self.assigned_to.name.split(' ').collect();

        if v.len() < 2 {
            panic!("Unable to read trigram")
        }

        format!(
            "rd-{number}-{trigram}-{version}-{subject}",
            number = self.id,
            subject = self.subject.replace(" ", "-").to_lowercase(),
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

    let client = reqwest::blocking::Client::new();

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

    let mut target_branch = format!("{}/{}", remote_name, "master");

    let head = repo.head()?;
    let head_ref = head.name().unwrap();

    if head_ref.ends_with(&ticket.issue.get_branch_name()) {
        println!(
            "We are already in the corresponding branch {}",
            ticket.issue.get_branch_name()
        );
        return Ok(());
    }

    println!("Target version : {}", ticket.issue.target_version());

    let target_branch_name = format!("origin/wab-{}", ticket.issue.target_version());

    let mut is_specific_target_branch = false;

    for b in repo.branches(Some(BranchType::Remote))? {
        let (b, _) = b?;
        let name = b.name()?;
        if name.unwrap_or("no name") == target_branch_name {
            is_specific_target_branch = true;
        }
    }

    if is_specific_target_branch {
        target_branch = target_branch_name;
    }

    for b in repo.branches(Some(BranchType::Remote))? {
        let (b, _) = b?;
        let name = b.name()?;

        if name.unwrap_or("no name") == target_branch {
            println!("I found {} !", name.unwrap_or("no name"));
            let reference = b.get();
            let name_new_branch = ticket.issue.get_branch_name();
            println!(
                "Let's create branch {} based on {}",
                name_new_branch, target_branch
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

fn main() {
    let opt = Opt::from_args();

    let body = get_ticket_body(opt.ticket, opt.key_redmine_api);

    let body = match body {
        Ok(body) => body,
        Err(e) => panic!("Unable to fetch Redmine API {}", e),
    };

    let ticket = read_issue(&body);

    let ticket = match ticket {
        Ok(t) => t,
        Err(e) => panic!("Unable to decode json \"{}\" => {}", body, e),
    };

    match create_new_branch(ticket) {
        Ok(()) => {}
        Err(e) => println!("Error : {}", e),
    }
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
                subject: String::from("Do stuff"),
                assigned_to: NamedProperty {
                    id: 220,
                    name: String::from("Arnold Bcon Tran"),
                },
                fixed_version: NamedProperty {
                    id: 318,
                    name: String::from("8.1.0"),
                },
                custom_fields: vec![NamedPropertyWithValue {
                    id: 50,
                    name: String::from("Developer"),
                    value: String::from("220"),
                }],
            },
        };
        assert_eq!(t.issue.get_branch_name(), "rd-42-abc-8.1-do-stuff");
    }
}