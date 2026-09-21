CALL {
  MATCH (v:DDVersion {id: '4.1.1'})
  RETURN count(v) AS release_count
}
CALL {
  MATCH (n:IMASNode {
    ids: 'equilibrium',
    id: 'equilibrium/ids_properties/version_put/data_dictionary'
  })
  RETURN count(n) AS stamp_count
}
CALL {
  MATCH (c:IMASNodeChange)
  WITH c,
       [(c)-[:FOR_IMAS_PATH]->(n:IMASNode) | n] AS owners,
       [(c)-[:IN_VERSION]->(v:DDVersion) | v] AS releases
  WHERE size(owners) = 1 AND size(releases) = 1
  RETURN count(c) AS change_count
}
RETURN CASE
  WHEN release_count = 1 AND stamp_count = 1 AND change_count > 0
  THEN 'imas_mvdd_smoke_ok'
  ELSE 'imas_mvdd_smoke_invalid'
END AS smoke_status,
release_count,
stamp_count,
change_count;
