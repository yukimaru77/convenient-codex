# rust-v0.156.1 への移行記録

2026-09-23 / 製品名 `convenient-codex`（便利な Codex）、独自改訂 `1`。

## 採用した公式版と移植元

**公式の最新安定版 `rust-v0.156.1` を基点に、`0.155.1+convenient.1` の独自差分全体を移植した。**
GitHub の latest release API と公式タグで確認した基点は
`b412ff32c417f855c2b2d1581b77058eed87c84b`。公開日時は 2026-09-23 02:41:36 UTC。
動き続ける upstream/main ではなく、このリリースに固定した。

- 作業場所: `/Users/nonaka/src/convenient-codex-0.156.1`（`main` の checkout とは別の worktree）
- 作業ブランチ: `port/rust-v0.156.1`
- 移植元: `main` の `d4f67ee5b9502c22ce0bdcb49f053a6b5038147b`（Port Convenient Codex onto rust-v0.155.1）と
  `00374552d5a3614cd30ea14e09fe574a765dd991`（0.155.1 の導入記録）
- 旧公式基点: `rust-v0.155.1` / `be2951ea34f0d295ed0becf97079f92fa5f6950e`

公式側の変更は 527 コミット、3,633 ファイル。独自差分は 171 ファイル・約 13,000 行。
`rust-v0.155.1` は `rust-v0.156.1` の祖先ではない（両者の分岐点は 2026-09-10 の `818f1cca8c`）ため、
独自差分を 3-way の cherry-pick で新しい基点へ適用した。

## 公式側の変更と独自機能の調整

競合は 18 ファイル。うち生成物 3 件（precomputed の app-server schema 2 件と `ThreadSettings.ts`）は
公式側を採用してから再生成した。残りは次のように統合した。

| 公式側の変更 | 独自機能への対応 |
| --- | --- |
| **環境選択の変更を次のターンまで遅延（#46310）。`TurnContext.environments` は `initial_environments` に改名** | **`env_switch` は既定の実行先を EnvironmentManager 側で管理し、ターンの環境スナップショットを書き換えないため影響なし。`env_status`・実行先の決定・テストは `initial_environments` を参照** |
| Windows sandbox の private desktop 設定を廃止し、`windows_sandbox_type` を追加（#46554 ほか） | 動的に作る実行環境の設定から旧フィールドを外し、ターン設定の `windows_sandbox_type` を引き継ぐ |
| ターンの起動理由を返す `current_turn_trigger()` を追加 | Goal の待機解除に渡す起動理由をこの API から取得し、analytics 用 trait への依存をやめた |
| `exec_command` に結果の tracing を追加 | Monitor 用のプロセス受け取り引数と統合し、`exec_monitor_command` を維持 |
| unified exec 必須時のツール除外と権限承認の条件を変更 | 公式の条件を維持したまま、`env_switch` 有効時の `environment_id` 公開を追加 |
| `spawn_agent` v2 の説明文から並行作業の一文を削除し、モデル定義による説明の差し替えを追加 | 公式の削除に従い、SSH・Docker 作業の委譲ガイドは維持 |
| AGENTS.md 管理の引数名と戻り値を変更 | 指示の読み込みをローカル環境に限る独自処理を新しい形に合わせた |
| セッション作成・tests・TUI の周辺に機能追加 | 独自のフィールド・テスト・監視表示を公式の追加部分と並べて統合 |

公式が 0.156.1 で追加したテストのうち、独自に拡張した構造体を作るもの 3 件
（Goal 拡張・MCP 拡張・TUI）には、独自フィールドの既定値を追加した。

### 移植時に確認した事項

- 公式 0.156.1 には、Monitor・`env_switch`・Goal 待機に相当する機能は追加されていない。
- `exec_command`・`apply_patch`・`view_image`・`request_permissions`・Monitor はすべて独自の実行先決定を経由する。
- multi-agent v2 の子エージェントは、親スレッドのキーを辿って env_switch の既定実行先を引き継ぐ。
  既存の結合テスト `spawned_agent_env_status_inherits_parent_thread_environment_cursor` がこの経路を検証している。
  v1 の `spawn_agent` だけが選択環境を並べ替えて渡す実装は、前回の移植から変えていない。
- GPT-6 系モデルは `spawn_agent` の説明文をモデル定義で差し替えられる。同梱のモデル定義と手元の
  models cache のどちらにも差し替えはなく、既定の説明文に委譲ガイドが入る。
- 0.155.1 から 0.156.1 の間に DB migration の追加・変更はない。既存 DB のまま導入・切戻しができる。

## 検証記録

完了済み:

- `cargo check --workspace --tests`（`codex-voice-host` を除く）が警告なしで通過。
  voice host は GStreamer の開発環境が必要で、前回と同じく対象外。
- 変更した 13 パッケージ（前回の 12 パッケージと `codex-mcp-extension`）の
  `cargo clippy --tests -- -D warnings` が警告なしで通過。
- stable / experimental の app-server schema、Python SDK、config schema を再生成。
  差分は前回の移植と同じファイル・同じ行数（Monitor・`goalWait`・実行環境 ID の追加分）。
- `just bazel-lock-update` を実行し、`MODULE.bazel.lock` の変更は不要だった。
- `just fmt`。

### Rust のテスト

変更した 13 パッケージの `just test` で 13,897 件を実行した。
初回は 13,820 件通過（19 件は再試行で通過）、50 件失敗、27 件時間切れ、45 件 skip。

**独自差分に起因した 15 件は修正して通過を確認した。**

| 対象 | 原因と対応 |
| --- | --- |
| scenarios のスナップショット 10 件 | 0.156.1 で追加された公式テスト。独自ツール（`env_switch`・`env_status`・`env_list`）、`environment_id` 引数、説明文、`exec_command` のエラー文の変更によりハッシュが変化。差分がこれらだけであることを確認して更新 |
| guardian のスナップショット 2 件 | 同上。code mode の `exec` に含まれるツール説明のハッシュだけが変化 |
| TUI の env_switch 表示 3 件 | 公式側でステータス行のモデル名が slug から表示名（`GPT-5.4`）に変わったため、独自テストの期待値を更新 |

scenarios のうち 2 件は、テストの一時ホームを使っても実ホームの `~/.agents/skills` を読み込み、
この Mac のユーザー skill がスナップショットに混入した（前回の記録にある skills-extension と同じ事象）。
スナップショットは `HOME` を一時ディレクトリにした状態で生成し、混入がないことを確認した。
この状態で scenarios と guardian の 21 件はすべて通過した。

残りの 63 件を 4 並列で再実行すると 36 件が通過した。まだ失敗した 27 件は次のとおり確認した。

| ケース | 確認結果 |
| --- | --- |
| TUI の非同期試験 5 件 | 負荷下で 60 秒の上限に到達。直列実行では 5 件とも 16〜29 秒で通過 |
| TUI の worktree_stack 1 件 | `target/debug/codex` が前回（0.155.1）のビルドのままだった。今回の checkout からビルドした `codex` で通過 |
| exec-server の registration retry 15 件 | 前回と同じ 500 ms 上限の事象。直列実行では移植版が 6 回とも全件通過。独自差分のない公式版も同条件で 9 回中 1 回失敗した |
| zsh fork の 4 件 | 直列実行では 3 件が通過。`subcommand_decline_marks_parent_declined_v2` は移植版・公式版とも 3 回中 3 回失敗 |
| MCP optional startup grace 1 件 | `zero_grace_respects_server_startup_timeout` が公式版でも同様に失敗 |
| seatbelt 内の openpty 1 件 | seatbelt 内で `xcrun` がキャッシュを作れず、Python が警告を出す。公式版でも同様に失敗 |

このほか、TUI の update_prompt のスナップショット 2 件は開発版の `0.0.0` を前提としており、
リリースタグの版 `0.156.1` と一致しない。公式版でも同様に失敗するため更新していない。

**最終的に失敗が残る 5 件は、いずれも独自差分のない公式 `rust-v0.156.1` でも同じ環境で同じように失敗する。**
比較には公式タグの別 worktree を使った。同じ target ディレクトリを共有すると、cargo がパス依存の
成果物を更新時刻だけで最新と判定し、両者の成果物が混ざることが分かった。
そのため混入の可能性がある比較結果は採用しなかった。移植版のソースの更新時刻を更新して作り直し、
その後に移植版側を再確認した。比較用の worktree は検証後に削除した。

利用者の指示により、今回は Docker の実機試験（`env_switch_docker_monitor_round_trip`）を行わず、
ローカルのテストだけで検証した。

ビルドキャッシュには既存の `/Volumes/CodexBuild20260912/target` を使った。
9 月 12 日以降使われていなかった `debug/incremental`（81 GB）を削除して空きを確保した。
V8 は公式パッケージ処理と同じ `rusty-v8-v150.4.0` の sandbox 版を、公式の SHA-256 manifest で検証して
`/Volumes/CodexBuild20260912/v8-cache` に置き、`RUSTY_V8_ARCHIVE` で指定した。

## 配布と版の識別

Cargo と CLI の公式版番号は `0.156.1` を保つ。リモート環境の準備時に、この版の公式リリースを
取得するためである。独自版は `0.156.1+convenient.1` として識別する。公式基点が変わったため
配置先は前回と重ならず、独自改訂は `1` のままにした。

### 導入した版と最終確認

**通常の `codex` コマンドを `0.156.1+convenient.1` に切替済み。**
ビルド対象のソースコミットは `282ef077f9fd498e019645d85df63e97467db924`。
未コミット変更のない状態で、この SHA を `STABLE_GIT_COMMIT` として指定した。
公式のパッケージ作成処理で CLI と code-mode-host を release profile（thin LTO）でビルドし、
33 分 33 秒で成功した。release でだけ出る警告 3 件（`codex-app-server` と `codex-cloud-tasks` の
`cfg(not(debug_assertions))` 部分）は、公式版から変更していないファイルのものだった。
この節を含む導入記録の追記は、その後の文書だけのコミットである。

- パッケージ: `~/.local/share/convenient-codex/releases/0.156.1+convenient.1`
- 選択先: `~/.local/share/convenient-codex/current`
- 起動コマンド: `~/.local/bin/codex` / `~/.local/bin/convenient-codex`
- 旧版: `releases/0.155.1+convenient.1` を残した。`current` をこちらへ戻せば切り戻せる
- 製品 metadata、ソース SHA、`source_dirty = false`、4 個の実行ファイルの SHA-256 と
  macOS コード署名を検証。
- bash / zsh の新規 login shell で `command -v codex` が上記ランチャーを指すことを確認。
- `codex --version` は `codex-cli 0.156.1`。features は `env_switch`・`monitor`・`goals` が
  すべて `true`。既存のログイン状態を維持。
- 同梱の code-mode-host の起動と、一時的な CODEX_HOME での app-server の `initialize` 応答と
  正常終了を確認。
- 実モデルを使ったローカルの `codex exec`（`--ephemeral`、一時ディレクトリ）で、`env_status`・
  `monitor` の開始・一覧・停止、`exec_command`、`apply_patch` が順に動き、独自ツールの定義が
  API に受け付けられることを確認した。

導入時点で旧版 0.155.1 の codex セッションが 5 個動いていた。これらは旧版のリリース
ディレクトリを絶対パスで実行しているため、切替の影響を受けない。新しく起動する CLI から新版になる。

前回と同じく、配布経路は Cargo ビルド。
