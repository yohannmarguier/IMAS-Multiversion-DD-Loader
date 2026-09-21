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
  MATCH (:IMASNodeChange)-[:FOR_IMAS_PATH]->(:IMASNode)
  RETURN count(*) AS change_count
}
RETURN CASE
  WHEN release_count = 1 AND stamp_count = 1 AND change_count > 0
  THEN 'imas_mvdd_smoke_ok'
  ELSE 'imas_mvdd_smoke_invalid'
END AS smoke_status;
