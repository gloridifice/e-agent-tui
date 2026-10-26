//! Independently owned Pi transport and protocol state for one live conversation.

use crate::{
    adapter::PiAdapter,
    process::{PiLaunchOptions, PiProcess},
    side::SideSessionControl,
};

pub struct PiSessionRuntime {
    pub process: PiProcess,
    pub adapter: PiAdapter,
    pub side: SideSessionControl,
}

impl PiSessionRuntime {
    pub async fn spawn(launch: &PiLaunchOptions) -> anyhow::Result<Self> {
        let adapter = PiAdapter::new(
            &launch.cwd,
            crate::session_index::project_session_root(&launch.cwd),
        )
        .with_compaction_model_path(crate::config::config_dir().join("compaction-model.json"));
        let process = PiProcess::spawn(launch).await?;
        Ok(Self {
            process,
            adapter,
            side: SideSessionControl::default(),
        })
    }

    pub fn prepare_side(
        &mut self,
        kind: e_tui::runtime::sessions::SideSessionKind,
        now: std::time::Instant,
    ) -> Result<crate::protocol::RpcCommand, String> {
        if !self.adapter.side_sessions_available() {
            return Err("The current Pi runtime does not support side session preparation".into());
        }
        self.side.begin(self.adapter.native_session_id(), kind, now)
    }
}
