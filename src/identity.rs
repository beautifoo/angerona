use crate::{details::is_noreply, push::strip_signature};

pub struct Ident {
    pub field: String,
    pub email: String,
    pub raw: String,
}

// `header` is every header line but the author, committer and tagger ones,
// such as a tag's name or a merged tag a mergetag header holds.
pub struct Object {
    pub idents: Vec<Ident>,
    pub header: Vec<u8>,
    pub message: Vec<u8>,
}

const TAG_SIGNATURES: [&[u8]; 3] = [
    b"-----BEGIN PGP SIGNATURE-----",
    b"-----BEGIN SSH SIGNATURE-----",
    b"-----BEGIN SIGNED MESSAGE-----",
];

// "Name <email>" as written: the email is what sits in the last <>.
fn ident(field: &str, raw: &str) -> Option<Ident> {
    let end = raw.rfind('>')?;
    let start = raw[..end].rfind('<')?;
    Some(Ident {
        field: field.to_string(),
        email: raw[start + 1..end].to_string(),
        raw: raw[..=end].to_string(),
    })
}

// A trailer line, "Key: Name <email>", as the message holds it.
fn trailer(line: &str) -> Option<Ident> {
    let (key, value) = line.split_once(": ")?;
    let mut chars = key.chars();
    if !chars.next()?.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let value = value.trim_end();
    let email = &value[value.rfind('<')? + 1..value.strip_suffix('>')?.len()];
    if email.is_empty() || email.contains('>') {
        return None;
    }
    Some(Ident {
        field: key.to_string(),
        email: email.to_string(),
        raw: value.to_string(),
    })
}

// Who an object names, as stored (never through a mailmap): its author,
// committer or tagger, and the trailers of its message, which is the rest
// after the headers; and its other headers. Both less the signature, whose
// base64 can spell a term by chance. Bytes are read lossily, so an odd or
// cut object gives less, not a panic.
pub fn read(raw: &[u8]) -> Object {
    let is_tag = raw.starts_with(b"object ");
    let signed = if is_tag {
        raw.to_vec()
    } else {
        strip_signature(raw)
    };
    let (headers, mut message) = match signed.windows(2).position(|w| w == b"\n\n") {
        Some(i) => (&signed[..=i], signed[i + 2..].to_vec()),
        None => (&signed[..], Vec::new()),
    };
    if is_tag {
        let mut kept = 0;
        for line in message.split_inclusive(|&b| b == b'\n') {
            if TAG_SIGNATURES.iter().any(|s| line.starts_with(s)) {
                break;
            }
            kept += line.len();
        }
        message.truncate(kept);
    }

    let mut idents = Vec::new();
    let mut header = Vec::new();
    for raw_line in headers.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(raw_line);
        let mut named = false;
        for field in ["author", "committer", "tagger"] {
            if let Some(rest) = line.strip_prefix(field).and_then(|r| r.strip_prefix(' ')) {
                idents.extend(ident(field, rest));
                named = true;
            }
        }
        if !named {
            header.extend_from_slice(raw_line);
            header.push(b'\n');
        }
    }
    for line in message.split(|&b| b == b'\n') {
        idents.extend(trailer(&String::from_utf8_lossy(line)));
    }
    Object {
        idents,
        header,
        message,
    }
}

// The people a push would name who are not the user: a tagger always, and the
// committer of a commit nobody has published yet (the author is kept, as a
// rebase or a patch keeps one). Noreply addresses carry nothing personal.
pub fn foreign<'a>(o: &'a Object, unpublished: bool, own: &[String]) -> Vec<&'a Ident> {
    let mut out = Vec::new();
    for i in &o.idents {
        let email = i.email.to_lowercase();
        let checked = i.field == "tagger" || (unpublished && i.field == "committer");
        if checked && !is_noreply(&email) && !own.iter().any(|e| e.to_lowercase() == email) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(o: &Object) -> Vec<(&str, &str)> {
        o.idents
            .iter()
            .map(|i| (i.field.as_str(), i.email.as_str()))
            .collect()
    }

    #[test]
    fn reads_headers_and_trailers() {
        let commit = b"tree 0\nauthor A <a@x.example> 1 +0000\ncommitter C <c@x.example> 1 +0000\n\nfix\n\nCo-authored-by: B <b@x.example>\nSigned-off-by: D <d@x.example>\n";
        assert_eq!(
            fields(&read(commit)),
            [
                ("author", "a@x.example"),
                ("committer", "c@x.example"),
                ("Co-authored-by", "b@x.example"),
                ("Signed-off-by", "d@x.example"),
            ]
        );
    }

    #[test]
    fn the_message_has_no_signature_in_a_commit() {
        let commit = b"tree 0\nauthor A <a@x.example> 1 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iQEzBAABCAAdFiEE\n -----END PGP SIGNATURE-----\n\nfix\n";
        assert_eq!(read(commit).message, b"fix\n");
    }

    #[test]
    fn the_message_has_no_signature_in_a_tag() {
        let tag = b"object 0\ntagger T <t@x.example> 1 +0000\n\nv1\n-----BEGIN SSH SIGNATURE-----\nU1NIU0lH\n-----END SSH SIGNATURE-----\n";
        assert_eq!(read(tag).message, b"v1\n");
    }

    #[test]
    fn a_truncated_object_does_not_panic() {
        assert_eq!(read(b"author \xff<").idents.len(), 0);
    }

    fn commit(author: &str, committer: &str) -> Object {
        read(
            format!("author X <{author}> 1 +0000\ncommitter Y <{committer}> 1 +0000\n\nm\n")
                .as_bytes(),
        )
    }

    #[test]
    fn own_committer_and_foreign_author_give_none() {
        let o = commit("bob@corp.example", "me@home.example");
        assert!(foreign(&o, true, &["me@home.example".to_string()]).is_empty());
    }

    #[test]
    fn a_foreign_committer_of_unpublished_gives_one() {
        let o = commit("me@home.example", "bob@corp.example");
        assert_eq!(foreign(&o, true, &["me@home.example".to_string()]).len(), 1);
    }

    #[test]
    fn a_foreign_committer_of_published_gives_none() {
        let o = commit("me@home.example", "bob@corp.example");
        assert!(foreign(&o, false, &["me@home.example".to_string()]).is_empty());
    }

    #[test]
    fn a_foreign_tagger_gives_one_either_way() {
        let o = read(b"object 0\ntagger T <bob@corp.example> 1 +0000\n\nv1\n");
        assert_eq!(foreign(&o, false, &[]).len(), 1);
    }

    #[test]
    fn a_noreply_committer_gives_none() {
        let o = commit("me@home.example", "1+me@users.noreply.github.com");
        assert!(foreign(&o, true, &[]).is_empty());
    }

    #[test]
    fn own_emails_compare_lowercased() {
        let o = commit("a@x.example", "Me@Home.Example");
        assert!(foreign(&o, true, &["me@home.example".to_string()]).is_empty());
    }
}
