-- Preserve fork/resume deferrals while replacing their referenced table.
CREATE TEMP TABLE saved_goal_deferrals AS
SELECT thread_id FROM thread_goal_continuation_deferrals;
DROP TABLE thread_goal_continuation_deferrals;
CREATE TABLE thread_goals_with_wait (
    thread_id TEXT PRIMARY KEY NOT NULL,
    goal_id TEXT NOT NULL,
    objective TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN (
        'active', 'goal_wait', 'paused', 'blocked',
        'usage_limited', 'budget_limited', 'complete'
    )),
    token_budget INTEGER,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    time_used_seconds INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
INSERT INTO thread_goals_with_wait SELECT * FROM thread_goals;
DROP TABLE thread_goals;
ALTER TABLE thread_goals_with_wait RENAME TO thread_goals;
CREATE TABLE thread_goal_continuation_deferrals (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES thread_goals(thread_id) ON DELETE CASCADE
);
INSERT INTO thread_goal_continuation_deferrals SELECT thread_id FROM saved_goal_deferrals;
DROP TABLE saved_goal_deferrals;
