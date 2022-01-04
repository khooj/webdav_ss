use futures_util::FutureExt;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use webdav_handler::fs::DavProp;

use super::{mem::Memory, PropStorage};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Prop {
    namespace: Option<String>,
    prefix: Option<String>,
    value: Option<Vec<u8>>,
    name: String,
}

#[derive(Clone)]
pub struct Yaml {
    filepath: PathBuf,
    mem: Memory,
}

impl Yaml {
    pub fn new(fp: PathBuf) -> Box<dyn PropStorage> {
        let mut m = Yaml {
            filepath: fp,
            mem: Memory::new_unboxed(),
        };

        if std::fs::metadata(&m.filepath).is_ok() {
            m.load().expect("can't load yaml props");
        }
        Box::new(m) as Box<dyn PropStorage>
    }

    fn load(&mut self) -> std::io::Result<()> {
        use std::io::{Error, ErrorKind};

        let mut f = std::fs::OpenOptions::new();
        let f = f.read(true).open(&self.filepath)?;
        let data: HashMap<String, Prop> =
            serde_yaml::from_reader(f).map_err(|e| Error::new(ErrorKind::Other, e))?;

        for (k, v) in &data {
            let _ = self
                .mem
                .add_prop(
                    &k.clone().into(),
                    (
                        true,
                        DavProp {
                            name: v.name.clone(),
                            namespace: v.namespace.clone(),
                            prefix: v.prefix.clone(),
                            xml: v.value.clone(),
                        },
                    ),
                )
                .map_err(|e| Error::new(ErrorKind::Other, e))?;
        }

        Ok(())
    }

    fn dump(&self) -> super::PropResult<()> {
        let data = self.mem.get_all_props();
        let data: HashMap<_, _> = data
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    Prop {
                        namespace: v.namespace,
                        name: v.name,
                        prefix: v.prefix,
                        value: v.xml,
                    },
                )
            })
            .collect();
        let mut opts = std::fs::OpenOptions::new();
        let f = opts.create(true).write(true).open(&self.filepath)?;

        serde_yaml::to_writer(f, &data).map_err(|_| webdav_handler::fs::FsError::GeneralFailure)
    }
}

impl PropStorage for Yaml {
    fn have_props<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
    ) -> super::PropFuture<bool> {
        async move {
            let r = self.mem.have_props(path).await;
            r
        }
        .boxed()
    }

    fn patch_prop<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
        patch: (bool, webdav_handler::fs::DavProp),
    ) -> super::PropFuture<super::PropResult<(hyper::StatusCode, webdav_handler::fs::DavProp)>>
    {
        async move {
            let r = self.mem.patch_prop(path, patch).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
        prop: webdav_handler::fs::DavProp,
    ) -> super::PropFuture<super::PropResult<Vec<u8>>> {
        async move {
            let r = self.mem.get_prop(path, prop).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
        do_content: bool,
    ) -> super::PropFuture<super::PropResult<Vec<webdav_handler::fs::DavProp>>> {
        async move {
            let r = self.mem.get_props(path, do_content).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn remove_file<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
    ) -> super::PropFuture<super::PropResult<()>> {
        async move {
            let r = self.mem.remove_file(path).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn remove_dir<'a>(
        &'a self,
        path: &'a crate::backend::normalized_path::NormalizedPath,
    ) -> super::PropFuture<super::PropResult<()>> {
        async move {
            let r = self.mem.remove_dir(path).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn rename<'a>(
        &'a self,
        from: &'a crate::backend::normalized_path::NormalizedPath,
        to: &'a crate::backend::normalized_path::NormalizedPath,
    ) -> super::PropFuture<super::PropResult<()>> {
        async move {
            let r = self.mem.rename(from, to).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }

    fn copy<'a>(
        &'a self,
        from: &'a crate::backend::normalized_path::NormalizedPath,
        to: &'a crate::backend::normalized_path::NormalizedPath,
    ) -> super::PropFuture<super::PropResult<()>> {
        async move {
            let r = self.mem.copy(from, to).await?;
            self.dump()?;
            Ok(r)
        }
        .boxed()
    }
}
