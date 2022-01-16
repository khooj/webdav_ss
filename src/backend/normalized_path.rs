use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use webdav_handler::davpath::DavPath;

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPath(String);

impl NormalizedPath {
    fn trim_token(mut token: &str) -> &str {
        if token.ends_with("/") {
            token = &token[..token.len() - 1];
        }
        if token.starts_with("/") {
            token = &token[1..];
        }
        token
    }

    pub fn join_file(&self, mut token: &str) -> NormalizedPath {
        token = NormalizedPath::trim_token(token);
        if token.len() == 0 {
            return self.clone();
        }
        let s = if self.0.ends_with("/") {
            format!("{}{}", self.0, token)
        } else {
            format!("{}/{}", self.0, token)
        };
        s.into()
    }

    pub fn join_dir(&self, mut token: &str) -> NormalizedPath {
        token = NormalizedPath::trim_token(token);
        if token.len() == 0 {
            return self.clone();
        }
        let s = if self.0.ends_with("/") {
            format!("{}{}/", self.0, token)
        } else {
            format!("{}/{}/", self.0, token)
        };
        s.into()
    }

    pub fn parent(&self) -> NormalizedPath {
        let s = if self.0.ends_with("/") {
            String::from_str(self.0.trim_end_matches("/")).unwrap()
        } else {
            self.0.clone()
        };
        let i = s.split("/");
        let len = s.split("/").count();
        i.take(len - 1)
            .fold(String::from_str("/").unwrap().into(), |acc, x| {
                acc.join_dir(x)
            })
    }

    pub fn strip_prefix(&self, prefix: &NormalizedPath) -> NormalizedPath {
        self.0.trim_start_matches(&prefix.0).into()
    }

    pub fn is_collection(&self) -> bool {
        self.0.ends_with("/")
    }

    pub fn is_root(&self) -> bool {
        self.is_collection() && self.0.len() == 1
    }

    pub fn dirs_parent(&self) -> NormalizedPath {
        if self.is_collection() {
            self.parent()
        } else {
            self.parent().parent()
        }
    }

    pub fn as_file(&self) -> NormalizedPath {
        if self.is_collection() {
            self.0.trim_end_matches("/").into()
        } else {
            self.clone()
        }
    }

    pub fn as_dir(&self) -> NormalizedPath {
        if !self.is_collection() {
            format!("{}/", self.0).into()
        } else {
            self.clone()
        }
    }
}

/// When creating from string we cant preserve ending slash for collections
/// so it is up to caller to do it.
impl From<String> for NormalizedPath {
    fn from(mut t: String) -> Self {
        if t.starts_with("/") && t.len() > 1 {
            t = String::from_str(&t[1..]).unwrap();
        }
        if t.len() == 0 {
            t = String::from_str("/").unwrap();
        }
        NormalizedPath(t)
    }
}

impl From<&str> for NormalizedPath {
    fn from(t: &str) -> Self {
        t.to_owned().into()
    }
}

impl From<&DavPath> for NormalizedPath {
    fn from(t: &DavPath) -> Self {
        let col = t.is_collection();
        let t = t
            .as_pathbuf()
            .strip_prefix("/")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let t = if col { format!("{}/", t) } else { t };
        NormalizedPath(t)
    }
}

impl From<DavPath> for NormalizedPath {
    fn from(t: DavPath) -> Self {
        (&t).into()
    }
}

impl Into<String> for NormalizedPath {
    fn into(self) -> String {
        self.0
    }
}

impl Into<Vec<u8>> for NormalizedPath {
    fn into(self) -> Vec<u8> {
        self.0.into()
    }
}

impl Deref for NormalizedPath {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NormalizedPath {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<str> for NormalizedPath {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl std::fmt::Display for NormalizedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::NormalizedPath;
    use webdav_handler::davpath::DavPath;

    #[test]
    fn create_from_string() {
        assert_eq!(NormalizedPath("/".into()), "/".into());
        assert_eq!(NormalizedPath("/".into()), "".into());
        assert_eq!(NormalizedPath("file.txt".into()), "/file.txt".into());
        assert_eq!(NormalizedPath("somedir/".into()), "/somedir/".into());
        assert_eq!(NormalizedPath("somedir/".into()), "somedir/".into());
        assert_eq!(
            NormalizedPath("somedir/file.txt".into()),
            "/somedir/file.txt".into()
        );
    }

    fn must_davpath(s: &str) -> NormalizedPath {
        DavPath::new(s).unwrap().into()
    }

    #[test]
    fn create_from_davpath() {
        assert_eq!(NormalizedPath("/".into()), must_davpath("/"));
        assert_eq!(NormalizedPath("file.txt".into()), must_davpath("/file.txt"));
        assert_eq!(NormalizedPath("somedir/".into()), must_davpath("/somedir/"));
        assert_eq!(
            NormalizedPath("somedir/file.txt".into()),
            must_davpath("/somedir/file.txt")
        );
    }

    #[test]
    fn joining() {
        let p: NormalizedPath = "/".into();
        assert_eq!(
            NormalizedPath("file/file/file".into()),
            p.join_file("file")
                .join_file("/file")
                .join_file("file/")
                .join_file("")
        );
        assert_eq!(
            NormalizedPath("dir/dir/dir/".into()),
            p.join_dir("dir")
                .join_dir("dir")
                .join_dir("dir")
                .join_dir("")
        );
    }

    #[test]
    fn parenting() {
        let p: NormalizedPath = "/some/long/directories/file.txt".into();
        assert_eq!(p.parent(), NormalizedPath("some/long/directories/".into()));
        assert_eq!(p.parent().parent(), NormalizedPath("some/long/".into()));
        assert_eq!(p.parent().parent().parent(), NormalizedPath("some/".into()));
        assert_eq!(
            p.parent().parent().parent().parent(),
            NormalizedPath("/".into())
        );
        assert_eq!(
            p.parent().parent().parent().parent().parent(),
            NormalizedPath("/".into())
        );
        assert_eq!(
            NormalizedPath("some/long/directories/".into()).parent(),
            NormalizedPath("some/long/".into())
        );
    }

    #[test]
    fn split_prefix() {
        let p: NormalizedPath = "/some/long/directories/file.txt".into();
        assert_eq!(
            p.strip_prefix(&"/some/long/directories/".into()),
            NormalizedPath("file.txt".into())
        );
        assert_eq!(
            p.strip_prefix(&"/some/long/".into()),
            NormalizedPath("directories/file.txt".into())
        );
        assert_eq!(
            p.strip_prefix(&"/some/".into()),
            NormalizedPath("long/directories/file.txt".into())
        );
        let p: NormalizedPath = "somekey.txt".into();
        assert_eq!(
            p.strip_prefix(&"/".into()),
            NormalizedPath("somekey.txt".into())
        );
    }
}
