DROP MATERIALIZED VIEW IF EXISTS <name_placeholder>;

CREATE MATERIALIZED VIEW <name_placeholder> AS
SELECT * FROM some_table;

REFRESH MATERIALIZED VIEW <name_placeholder>;