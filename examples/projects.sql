-- Synthetic project data only. Create a disposable DB with:
-- sqlite3 /tmp/clue-demo.db < examples/projects.sql
CREATE TABLE projects (name TEXT PRIMARY KEY, language TEXT NOT NULL);
CREATE TABLE issues (id INTEGER PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(name), title TEXT NOT NULL, description TEXT NOT NULL, status TEXT NOT NULL);
INSERT INTO projects VALUES ('Alchemy','Rust + TypeScript'),('Cider','Rust'),('Cortex','Rust'),('AgentKernel','Rust');
INSERT INTO issues VALUES
(1,'Alchemy','Share notebooks across devices','Offline notebook edits must converge without losing sources or resurrecting deleted notes.','open'),
(2,'Cider','Calendar search window','Find upcoming project meetings with start and end dates.','open'),
(3,'Cortex','Retrieve decisions from memory','Rank bounded evidence for a project handoff; preserve reference identities.','open'),
(4,'AgentKernel','Isolated build environment','Run untrusted test code in a disposable sandbox and clean up processes.','open'),
(5,'Alchemy','Notebook theme colors','Adjust notebook appearance and syntax highlighting.','done');
