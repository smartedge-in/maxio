//! Storage Raft Prometheus helpers (P1-17).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageRaftMetrics {
    pub leader_node: Option<u64>,
    pub commit_lag: u64,
}

impl StorageRaftMetrics {
    pub fn render_prometheus(&self) -> String {
        let leader = if self.leader_node.is_some() { 1 } else { 0 };
        format!(
            "# HELP raft_storage_leader 1 when this node observes a storage Raft leader\n\
             # TYPE raft_storage_leader gauge\n\
             raft_storage_leader {leader}\n\
             # HELP raft_storage_commit_lag Storage Raft commit lag (last log index - applied)\n\
             # TYPE raft_storage_commit_lag gauge\n\
             raft_storage_commit_lag {}\n",
            self.commit_lag
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_lines_present() {
        let m = StorageRaftMetrics {
            leader_node: Some(1),
            commit_lag: 2,
        };
        let out = m.render_prometheus();
        assert!(out.contains("raft_storage_leader 1"));
        assert!(out.contains("raft_storage_commit_lag 2"));
    }
}
