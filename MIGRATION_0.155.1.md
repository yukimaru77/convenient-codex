# rust-v0.155.1 への移行記録

2026-09-19 / 製品名 `convenient-codex`（便利な Codex）、独自改訂 `1`。

## 採用した公式版と保存した旧版

**公式の最新安定版 `rust-v0.155.1` を基点に、旧版の独自機能全体を移植した。**
GitHub の latest release API と公式タグで確認した基点は
`be2951ea34f0d295ed0becf97079f92fa5f6950e`。公開日時は 2026-09-18 20:03:04 UTC。
動き続ける upstream/main ではなく、このリリースに固定した。

- 新しい作業場所: `/Users/nonaka/src/convenient-codex`
- 移植元: `/Users/nonaka/tasks/codex-monitor-v0.153.4`
- 保存ブランチ: `archive/custom-0.153.4`
- 保存コミット: `824932436f78444f0376045c62b68f105391d877`
- 旧公式基点: `rust-v0.153.4` / `3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`

保存版には旧公式基点からの独自コミット 11 個に加え、旧作業場所にあった未コミット・
未追跡の実装も含めた。旧リポジトリと旧インストール先は復旧用に残す。
新しいリポジトリの `upstream` は OpenAI、`legacy` は旧 `codex-switch` を指す。

## 維持・調整した機能

| 対象 | 移植内容 |
| --- | --- |
| **環境切替** | **`env_switch`、`env_status`、`env_list`。SSH・Docker・多段接続、ツールの実行先、子エージェントへの環境継承を維持** |
| Monitor | 定期的な先頭 3 行・末尾 20 行の通知、無出力時の静止、終了通知、Realtime、TUI の監視表示を維持 |
| Goal | Monitor の存在だけでは止めない。最終応答の `GOAL_WAIT` と稼働中 Monitor による待機、外部ターンによる復帰、状態保存を維持 |
| 研究継続 | 独自の継続プロンプトを維持し、upstream の明示的な pause の扱いを統合 |
| 最新版との整合 | workspace roots、リクエストごとの環境設定、delegate isolation、空応答による Goal の停止処理、音声・推論表示などの upstream の追加仕様を維持 |
| 製品管理 | 製品 manifest、読み取り専用の最新版確認、版別ディレクトリに配置する専用インストーラーを追加 |

確認中に、commentary の `GOAL_WAIT` と空の final の組合せで誤って待機する既存不具合を発見。
Goal が実際の応答 phase を記録して判定するよう修正し、回帰テストを追加した。
また、実行先を解決するときに request 固有の cwd・権限設定を保持するテストを追加した。
リモートの `/bin/sh` 向けに行っていた調整がローカルにも及んでいたため、
ローカルでは従来どおり login shell を使えるように修正した。

## 検証記録

完了済み:

- CLI と `codex-code-mode-host` のビルド。
- stable / experimental の app-server schema と Python SDK の再生成。
- config schema の再生成、`just bazel-lock-update`。
- 更新確認スクリプトの実 API 確認と成功・異常系 9 ケース。
- インストーラーの一時ディレクトリ内での動作確認 7 項目。
- 公式パッケージ作成処理による補助実行ファイルの組み込み。
- 変更した 12 パッケージの `just fix`、残る警告を修正した app-server / state の
  `just clippy -- -D warnings`、最終 `just fmt`。
- Goal の DB migration 試験を最新版の共通 `SqliteConfig` 接続に合わせ、単独でも通過。

### Rust と実機の確認

変更した 12 パッケージの `just test` で 12,538 件を実行した。
初回は 12,387 件通過、135 件失敗、16 件時間切れ。必要な `test_stdio_server` を
ビルドし、テストの版表示・ツール一覧・履歴比較を修正してから、失敗した全ケースと
変更した独自機能を再確認した。色を検査する TUI テストでは `NO_COLOR` を解除した。

- **Goal / Monitor の統合試験は通過。** 待機・継続、実 Monitor の終了やユーザー入力による
  復帰、静かな監視中にモデルを呼ばないこと、通知の先頭・末尾保持と状態保存を確認。
- Docker の opt-in 実機試験 `env_switch_docker_monitor_round_trip` は **10.323 秒で通過**。
  Debian の使い捨てコンテナへ公式バイナリを準備し、remote `apply_patch`・`exec_command`・
  Monitor 終了通知・local 復帰まで確認。試験用コンテナは削除し、Colima は元の停止状態、
  Docker の既定 context は `desktop-linux` に戻した。
- 絞り込んだ再検証は 280 件中 260 件通過。その後、修正した履歴比較と TUI の残るケースも通過。
  履歴比較は developer のスキル説明にある `second opinions` を削除対象の発言と誤認していたため、
  user メッセージの構造・本文を完全一致で検査するようにした。

### テストランナーで残った制約

**一括の `just test` が全件成功した、という結果ではない。** 次の upstream 由来のケースは
macOS で実行方法・初回起動の影響を受けた。製品の待ち時間や既存テストの上限は緩めていない。

| ケース | 確認結果 |
| --- | --- |
| exec-server の registration retry | nextest のプロセス分離では 500 ms の上限により 15 件失敗。同じ既存テストバイナリで 16 件を連続実行すると全件通過（2.81 秒）。内部の遅延箇所は特定できていない |
| optional MCP startup grace | 250 ms の起動上限を使うケースは nextest で初期化開始前に失敗。同一プロセスで 4 ケースを実行すると対象は通過し、失敗は最初のケースへ移った（3 通過・1 失敗） |
| shell snapshot の資格情報検査 | nextest の 60 秒上限に到達。既存バイナリから同じケースを直接実行すると 48.912 秒で自然完了・通過 |
| TUI の一部非同期試験 | macOS FSEvents の停止待ちを実測。再検証と単独実行で通過。新しい実行ファイルのコピーも初回起動 7.257 秒・再起動 0.019 秒と差があった |
| zsh subcommand の Cancel | `Declined` を期待する試験で `Failed`。upstream から未変更の Cancel → turn interrupt → 親シェル終了という経路とログが一致。今回の移植コードはこの条件の振る舞いを変えていないため未修正 |
| managed daemon の updater 復旧 | readiness socket が 10 秒以内に作成されず、updater の検査前に失敗。CLI / daemon の実装は公式版と同一。別ボリュームから大きな実行ファイルをコピーする fixture と初回起動の遅さは確認できたが、子プロセス内部の停止箇所は未特定 |

ワークスペースの残りも `just test --workspace`（上記 12 パッケージは重複実行を除外）で
検証を試みたが、`codex-voice-host` が要求する `pkg-config` / GStreamer 開発環境が
この Mac にないため、`glib-sys` のビルドで停止した。これは upstream の私的な音声ヘルパーの
基盤であり、通常の CLI にはまだ接続されていない（`voice-host/README.md`）。
このパッケージを追加で除いた確認と導入確認の結果は後段に追記する。

ローカルの詳細ログは `.git/migration-*.log` に保存している。
ビルドキャッシュは既存の `/Volumes/CodexBuild20260912/target` を使用し、
`CARGO_INCREMENTAL=0`、dev / test の debug 情報を無効にしてディスク消費を抑えた。
今回のローカル導入用バイナリは dev profile で作成する。最適化した release profile での
再ビルド手順は README に記載している。

### データの保全

導入前の DB を
`~/.local/share/convenient-codex/backups/pre-0.155.1-20260919T090127Z/`
に保存した。対象は state・thread_history・goals・queue・memories の 5 DB。
前 4 件は SQLite backup API で保存し、memories は read lock 下で一貫したコピーを取得して
コピー側の `integrity_check = ok` を確認した。内訳は同ディレクトリの `manifest.json` に記録。
ディレクトリは 0700、ファイルは 0600。診断ログ DB は対象外で、認証・設定は変更していない。

## 配布と版の識別

Cargo と CLI の公式版番号は `0.155.1` を保つ。リモート環境の準備時に取得する
公式リリースを解決するためである。独自版は `0.155.1+convenient.1` として識別し、
導入時にソースコミットと各実行ファイルの SHA-256 を保存する。

専用の導入先は `~/.local/share/convenient-codex/releases/`。
`current` symlink が選択中の版を指し、`~/.local/bin/convenient-codex` から起動する。
`--set-default` で `~/.local/bin/codex` も作成できる。
`codex --build-info` で導入した改造版を識別する。

本移行の配布経路は Cargo ビルド。Bazel で作る開発用バイナリは upstream と同じく
版が `0.0.0` になり、リモート準備時の `HostVersion` は最新公式版にフォールバックする。
追加ソース・Goal の SQL migration・テンプレートは既存の Bazel glob / compile_data で
取得されることも確認した。Bazel での全ビルドは今回の検証対象には含めていない。

詳細な製品仕様は [CUSTOM_CODEX_SPEC.md](CUSTOM_CODEX_SPEC.md)、
再ビルド・導入手順は [README.md](README.md) を参照。
