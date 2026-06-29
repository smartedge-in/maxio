//! P1-16: minimal OpenRaft dependency gate — full cluster logic is P1-17.

#[cfg(test)]
mod tests {
    #[test]
    fn openraft_crate_links() {
        let _ = openraft::Config::default();
    }
}