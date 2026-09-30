# 便利な Codex — convenient-codex

**同じ会話でローカル・SSH・Docker を行き来し、長い処理を監視しながら Goal を進めるための個人版 Codex。** [OpenAI Codex](https://github.com/openai/codex) を土台に、次の機能を加えています。

| 機能 | できること |
| --- | --- |
| **実行環境の切替** | **`env_switch` で SSH・Docker・その入れ子へ移動。コマンド、パッチ、画像確認、監視に同じ実行先を使う** |
| 環境の確認 | `env_status` / `env_list` で既定実行先と登録済み環境を確認する |
| 定期的な監視 | `monitor` で出力の先頭 3 行・末尾 20 行を通知。既定は 60 分ごとで、無出力なら通知せず、終了時はすぐ結果を届ける |
| 即時の監視 | `monitor_realtime` で即時性が必要な出力を通知する |
| Goal の待機と復帰 | 最終応答の `GOAL_WAIT` と同じセッションの Monitor により待機し、新しいユーザー入力や監視通知で再開する |
| 状況表示 | TUI に実行環境、監視件数・周期、Goal 状態を表示する |
| **音声会話** | **`/voice` 用の `codex-voice-host` と pinned GStreamer／GLib runtime を macOS ARM64 package に同梱する** |

Monitor があるだけでは Goal を止めません。Goal の継続プロンプトは、研究成果を縮小せず、有用な作業を進め、結果待ちだけになったら休む方針です。細部・制約・受け入れ条件は [製品仕様](CUSTOM_CODEX_SPEC.md) にまとめています。

## 現在の公式基点と製品識別

| 項目 | 値 |
| --- | --- |
| 製品名 | `convenient-codex`（便利な Codex） |
| 独自改訂 | `2` |
| 公式基点 | [`rust-v0.159.2`](https://github.com/openai/codex/releases/tag/rust-v0.159.2) |
| 公式コミット | `ff6aec96948b70d94983af2641a6b67c94faeff5` |
| 旧版の保存 | `archive/custom-0.153.4` — `824932436f78444f0376045c62b68f105391d877` |
| 移植の確認状況 | ビルド・テスト・導入の結果は [移行記録](MIGRATION_0.159.2.md) に記録する |

[CONVENIENT_CODEX.json](CONVENIENT_CODEX.json) が、製品名・独自改訂・公式タグとコミット・移植元を記録する機械可読の識別情報です。ソースの変更を特定するときは、この情報と Git コミットを使います。

**Cargo のパッケージ版と `codex --version` は、意図的に公式の `0.159.2` を維持します。** `env_switch` がリモート用バイナリを準備するときに、この版から公式リリースを選ぶためです。独自改訂は JSON で別管理し、存在しない独自リリースを取得しに行かないようにします。`--version` だけでは改造版と公式版を区別できません。

## この checkout からビルド・導入する

必要な Rust toolchain と補助ツールは [ビルド手順](docs/install.md) を参照してください。CLI には `codex-code-mode-host` などの補助ファイルも必要なため、公式の [パッケージ作成処理](scripts/codex_package/README.md) でまとめてビルドします。ビルド・導入の Python は 3.10 以降が必要です。以下はこの Mac の Python 3.12 を使う例で、リポジトリのルートから実行します。

```bash
STABLE_GIT_COMMIT="$(git rev-parse HEAD)" CODEX_REPO_ROOT="$PWD" \
  python3.12 scripts/build_codex_package.py \
  --variant codex --cargo-profile release \
  --package-dir /tmp/convenient-codex-package

python3.12 scripts/convenient-install.py \
  --entrypoint-bin /tmp/convenient-codex-package/bin/codex \
  --code-mode-host-bin /tmp/convenient-codex-package/bin/codex-code-mode-host \
  --rg-bin /tmp/convenient-codex-package/codex-path/rg \
  --zsh-bin /tmp/convenient-codex-package/codex-resources/zsh/bin/zsh

~/.local/bin/convenient-codex --build-info
~/.local/bin/convenient-codex
```

[専用インストーラー](scripts/convenient-install.py) は既にビルドしたバイナリを受け取り、補助ファイルを含む版別パッケージを組み立てます。Linux ではビルドと導入の `--target` を揃え、`--bwrap-bin /tmp/convenient-codex-package/codex-resources/bwrap` も渡してください。ローカルへの導入は macOS / Linux が対象です。

配布用には変更をコミットしてからビルドします。`STABLE_GIT_COMMIT` は upstream の仕組みで実行ファイルにもコミットを記録する指定です。

既定の配布先は `~/.local/share/convenient-codex/releases/0.159.2+convenient.2`、選択中の版は `~/.local/share/convenient-codex/current` です。`~/.local/bin/convenient-codex` から選択中の版を起動します。導入時の `build-info.json` に製品 manifest、導入した checkout の Git コミット、未コミット変更の有無、バイナリの SHA-256 を残し、ランチャーの `--build-info` で表示します。

macOS ARM64 の音声 helper は、公式の pinned native recipe を使って `bazel build //third_party/voice:native_prefix` と `bazel build //third_party/voice:native_runtime` を実行し、`third_party/voice/assemble_package.py` で package に追加します。helper は package 内の `@loader_path/../lib` へリンクを固定し、runtime の `Hello`・`InitializeRuntime`・`Close` を検証してから導入します。音声 runtime の生成物はホストの Homebrew ライブラリを実行時に参照しません。

普段の `codex` コマンドにも使う場合は、導入コマンドに `--set-default` を付けます。これは `~/.local/bin/codex` に同じランチャーを作る指定で、PATH は変更しません。シェルの設定で `~/.local/bin` を npm の実行先より前に登録し、現在のシェルでは次のように確認します。

```bash
export PATH="$HOME/.local/bin:$PATH"
hash -r
command -v codex
codex --build-info
codex --version
```

`command -v codex` が `~/.local/bin/codex` を指し、`--build-info` の `product.name` が `convenient-codex`、`installation.version` が `0.159.2+convenient.2` なら、この版のランチャーを使っています。`--version` は `codex-cli 0.159.2` と表示します。`--build-info` は専用ランチャーの引数で、パッケージ内の `bin/codex` を直接実行する場合には使えません。

既存の npm パッケージとその `codex` は残します。npm 版へ戻すときは PATH の順序を元に戻すか、専用インストーラーが作った `~/.local/bin/codex` だけを退避し、`hash -r` の後に `command -v codex` を確認します。元の npm 版は `"$(npm prefix -g)/bin/codex" --version` でも直接確認できます。`~/.local/bin/convenient-codex` と版別パッケージは、そのまま残して併用できます。

自分が管理していない同名ランチャーや symlink は上書きせず停止します。同じ独自改訂に異なる内容を入れ直す場合も拒否するため、変更版を導入するときは manifest の `product.patch_revision` を増やします。

既存の会話・認証・設定を利用します。`env_switch` は既定で有効、Monitor は `monitor = true` で有効になります。既存の `~/.codex/config.toml` の `[features]` に統合してください。

```toml
[features]
env_switch = true
monitor = true
```

この版の導入検証の結果は [移行記録](MIGRATION_0.159.2.md) を参照してください。通常の `npm install -g @openai/codex`、Homebrew、OpenAI の installer は公式版を導入する手段です。この checkout の独自機能を配布するものではありません。デスクトップアプリ内蔵の実行ファイルも別です。

## 公式の安定版との差を確認する

Python 3.9 以降の標準ライブラリだけで実行できます。カレントディレクトリに関係なく、スクリプトのあるリポジトリの manifest を読みます。

```bash
python3 scripts/convenient-check-upstream.py
```

GitHub の公式 latest release API と `CONVENIENT_CODEX.json` を比較し、採用中の公式タグ、最新安定版、比較結果、リリース URL を表示します。更新候補があっても成功終了し、取得失敗・不正なメタデータは説明を出して非ゼロで終了します。

これは確認だけの処理です。ビルド、インストール、定期実行、Git の変更、push、外部への通知は行いません。更新候補を取り込み検証する際の要件は [製品仕様](CUSTOM_CODEX_SPEC.md) にあります。

## 仕様と upstream の資料

- **[便利な Codex の製品仕様](CUSTOM_CODEX_SPEC.md)** — 維持する機能、制約、更新時の受け入れ条件
- [製品識別情報](CONVENIENT_CODEX.json)
- [実行環境切替の移植元記録](ENV_SWITCH.md) / [Monitor の移植元記録](MONITOR.md) — 過去の版の検証記録を含む
- [公式 Codex ドキュメント](https://developers.openai.com/codex) / [認証](https://developers.openai.com/codex/auth)
- [upstream の開発・貢献ガイド](docs/contributing.md)

OpenAI Codex 由来の [Apache-2.0 License](LICENSE) と [NOTICE](NOTICE) を継承します。
