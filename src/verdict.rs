use crate::details::is_address;

// The email git uses outside any repo: one, none set (git exits 1 and prints
// nothing), or unknown because git failed some other way.
pub enum Outside {
    Set(String),
    Unset,
    GitFailed,
}

// What git says about your email here, lowercased, read once by
// details::email_facts.
pub struct EmailFacts {
    pub repo_email: Option<String>,
    pub outside: Outside,
}

// The repo's own email: the one git uses in this repo, which that repo is
// meant to carry, when it is shaped like an address and differs from the one
// git uses outside any repo, which is never skipped. Any repo email differs from none set. A user.email of a bare
// login would hide the finding it equals, and when git failed outside, none
// is skipped, so more is reported, not less.
pub fn repo_own_email(f: &EmailFacts) -> Option<String> {
    let repo = f.repo_email.as_ref().filter(|e| is_address(e))?;
    match &f.outside {
        Outside::Set(o) if o == repo => None,
        Outside::Set(_) | Outside::Unset => Some(repo.clone()),
        Outside::GitFailed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_email(repo: Option<&str>, outside: Outside) -> Option<String> {
        repo_own_email(&EmailFacts {
            repo_email: repo.map(str::to_string),
            outside,
        })
    }

    #[test]
    fn the_repo_email_is_its_own_only_when_it_differs_from_outside() {
        let set = |e: &str| Outside::Set(e.to_string());
        let got = [
            own_email(Some("me@work.example"), set("me@home.example")),
            own_email(Some("me@home.example"), set("me@home.example")),
            own_email(Some("me@work.example"), Outside::Unset),
            own_email(Some("me@work.example"), Outside::GitFailed),
            own_email(Some("jdoe"), Outside::Unset),
            own_email(None, Outside::Unset),
        ];
        assert_eq!(
            got,
            [
                Some("me@work.example".to_string()),
                None,
                Some("me@work.example".to_string()),
                None,
                None,
                None,
            ]
        );
    }
}
