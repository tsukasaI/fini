# 判断待ち操作の一覧

調査日: 2026-11-10。どの操作も実行していません。指摘 ID は `audit/REPORT.md` の番号です。

使い方:
- 新規 issue とコメントのコマンドは `--body-file` を使います。各項目の「投稿本文」(4 連のバッククォートで囲んだ部分の中身)をファイルに保存し、そのパスを `<本文ファイル>` に入れてください。
- 新規 issue の「調査時点の状態」は、同じ趣旨の issue が open / closed のどちらにも無かったことを指します(検索語は REPORT.md の各指摘に記載)。
- 本文中の再現手順は実行していません(バイナリの直接実行が拒否されたため)。投稿前に一度実行して、結果を本文に反映することを勧めます。
- 既存 issue(#91 など)は証拠の行で区切りにダッシュ記号を使っていますが、このリポジトリの書き込みフックがその記号を禁止しているため、下書きではコロンにしています。

## 実行状況(2026-11-10 追記)

利用者の承認(13 件すべて)を受けて実行しました。リモートの main(`ef38390`、#258)を取り込み、行番号と各項目の「投稿前の再確認」を済ませています。#258 はテスト内の追記のみで、引用している行番号に影響はありません。

| 項目 | 状態 | 結果 |
|---|---|---|
| A-01 | 実行済み | #264。単体テストと結合テストで再現を確認し、本文を「確認済み」に更新して投稿 |
| A-02 | 実行済み | #265。同上。修正方針が決まったため `needs-decision` ラベルは付けていない |
| A-03 | 実行済み | #266。実行結果(伏せ字にならない 2 行を含む diff)を本文に引用 |
| A-04 | 実行済み | #267。実行で確認した 9 行と、読解のみの接頭辞を本文で区別 |
| A-05 | 実行済み | #268。実行結果を本文に反映。参照先は #264 |
| A-06 | 実行済み | #259 |
| A-07 | 実行済み | #260 |
| A-08 | 実行済み | #261 |
| A-09 | 実行済み | #241 にコメント(issuecomment-6766634227)。再オープンはしていない |
| A-10 | 実行済み | #262。行番号 3 箇所を実ファイルに合わせて修正(`release.yaml:21`、`:23`、`:135`、`.pre-commit-config.yaml:18`) |
| A-11 | 実行済み | #263。行番号の基準を `ef38390` に更新 |
| A-12 | 実行済み | #269。挙動 7 項目のうち 5 項目を実行で確認し、本文に結果を記載 |
| A-13 | 実行済み | PR #153、#168、#197 をコメント付きでクローズ。#168 は応答待ちでタイムアウトしたが、クローズとコメントの反映を確認済み |

下の各項目の本文は下書き時点のものです。実際に投稿した本文との差分は上の表のとおりです。

---

## - [x] A-01 新規 issue: ファイル直接指定で除外が効かない(B-01)

- 対象: 新規
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)
- 投稿前の再確認: 再現手順を実行して結果を確かめ、同趣旨の issue が新しく立っていないか `gh issue list --search "exclude"` で確認する。
- コマンド:
  ```bash
  gh issue create --title "[high] Exclude patterns are not applied to files passed explicitly (pre-commit and Claude hook paths)" --label bug --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

`--exclude` と `fini.toml` の `exclude` は、ディレクトリを走査して見つけたファイルにしか適用されない。引数で直接指定したファイルには適用されない。

README が案内する連携のうち、pre-commit フック(`types: [text]` でファイル名を渡す)と Claude Code フック(編集したファイル 1 件を渡す)はどちらも直接指定になるため、`exclude = ["vendor/"]` と設定していても `vendor/` 配下のファイルが書き換えられる。

**深刻度: high**(利用者が明示的に除外したファイルを修正モードが書き換える)

# 証拠

- `src/walker.rs:64`: `WalkBuilder::new(path)`
- `src/walker.rs:94-110`: 除外パターンは `builder.overrides(...)` に渡すだけで、起点への独自チェックはない
- `ignore` 0.4.33 `src/walk.rs:1149-1152`: 起点(深さ 0)は無条件で通す

```rust
fn skip_entry(&self, ent: &DirEntry) -> Result<bool, Error> {
    if ent.depth() == 0 {
        return Ok(false);
    }
```

除外の結合テスト(`tests/integration.rs:979`、`:998`、`:1019`)はディレクトリ指定のみ。

# 再現手順

```console
$ mkdir -p t/vendor && printf 'x \n' > t/vendor/a.txt
$ fini --check --exclude 'vendor/' t                # 除外される
$ fini --check --exclude 'vendor/' t/vendor/a.txt   # 除外されず exit 1 になる(コードからの予測)
```

# 影響範囲

- pre-commit / prek 経由、Claude Code フック経由、`fini path/to/file` の全ケース
- 修正方針: 起点がファイルのときも除外パターンを照合する(起点ごとに `Override::matched` を呼ぶ)。意図した仕様なら README と `--exclude` のヘルプに明記する
- 隠しファイルと `.gitignore` が直接指定で無視されるのは意図どおりと理解している。この issue の対象は利用者が書いた除外パターンのみ
````

---

## - [x] A-02 新規 issue: ゼロ幅文字の除去が絵文字と一部言語を壊す(B-02)

- 対象: 新規
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)
- 投稿前の再確認: 再現手順を実行して結果を確かめる。仕様として維持する判断なら、ラベルと本文の修正方針を調整する。
- コマンド:
  ```bash
  gh issue create --title "[high] Default zero-width removal strips ZWJ/ZWNJ/LRM/RLM and corrupts emoji sequences and non-Latin text" --label bug --label needs-decision --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

既定で有効なゼロ幅文字の除去は、U+200D(ゼロ幅接合子)、U+200C(ゼロ幅非接合子)、U+200E / U+200F(書字方向の指定)を、文脈を見ずにファイル中のすべてから取り除く。

- U+200D は絵文字の合成に必須(技術者、レインボーフラッグ、家族などの絵文字は、複数の絵文字を U+200D でつないだもの)
- U+200C はペルシア語、U+200D はシンハラ語などで綴りの一部
- U+200E / U+200F は左右混在の文章で表示順を決める

README、Markdown、翻訳ファイル、テストデータにこれらが含まれると、修正モードで黙って別の文字列になる。

**深刻度: high**(既定の動作で内容が変わり、警告は「zero-width character」としか出ない)

# 証拠

- `src/normalize/fix.rs:5-13`: `ZERO_WIDTH_CHARS` に U+200C / U+200D / U+200E / U+200F を含む
- `src/normalize/fix.rs:246-273`: `ZERO_WIDTH_CHARS.contains(&ch)` のみで判定(例外は先頭の BOM だけ)
- `src/normalize/mod.rs:50`: `remove_zero_width: true` が既定
- `src/normalize/mod.rs:748`: 既存テストは `"a\u{200D}b\n"` からの除去を固定。絵文字や非ラテン文字の例はない

# 再現手順

```console
$ printf '\xf0\x9f\x91\xa8\xe2\x80\x8d\xf0\x9f\x92\xbb\n' > e.md   # U+1F468 U+200D U+1F4BB
$ fini e.md
$ od -c e.md    # U+200D(e2 80 8d)が消えている(コードからの予測)
```

# 影響範囲

- 既定設定で修正モードを通るすべてのテキストファイル
- 修正方針の案:
  - A. 既定の除去対象を U+200B、U+2060、ファイル途中の U+FEFF に絞り、U+200C / U+200D / U+200E / U+200F は別オプションにする
  - B. 前後が絵文字または該当する文字体系のときは残す
  - C. 現状維持とし、README に影響と `--keep-zero-width` の案内を明記する
````

---

## - [x] A-03 新規 issue: 複数行の秘密鍵が diff で伏せ字にならない(B-03)

- 対象: 新規(関連: #44、#156)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。#44 は closed(最終更新 2026-07-17T13:30:36Z)、#156 は closed(最終更新 2026-10-13T01:26:00Z)
- 投稿前の再確認: ダミーの鍵で再現手順を実行して結果を確かめる。本物の鍵は使わない。
- コマンド:
  ```bash
  gh issue create --title "[medium] --diff masks only the BEGIN line of a multi-line private key; the key body is printed" --label bug --label security --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

diff の伏せ字は 1 行単位で、秘密情報のパターンに一致した行だけを置き換える。PEM 形式の秘密鍵で一致するのは `-----BEGIN ... PRIVATE KEY-----` の行だけなので、鍵の本体(Base64 の行)は伏せ字にならない。

鍵ファイルの改行コードが CRLF、または行末に空白がある場合、`--diff` は全行を変更として表示するため、本体が CI ログに出る。変更行の前後 3 行の文脈としても表示される。

**深刻度: medium**(#44 の「diff に生の値を出さない」という契約の抜け。秘密鍵として検出はされているのに本体が出る)

# 証拠

- `src/normalize/detect.rs:32`: 一致するのはヘッダー行のみ
- `src/normalize/detect.rs:269-300`: `mask_secret_lines` は `split_inclusive('\n')` の各行を独立に判定
- `src/output.rs:449`: `grouped_ops(3)` で前後 3 行を表示
- `CLAUDE.md`: "all diff paths mask secret-matching lines"
- `README.md:175-178`: 注意書きは「どのパターンにも一致しない秘密」についてのみ

# 再現手順

```console
$ printf -- '-----BEGIN RSA PRIVATE KEY-----\nFAKEFAKEFAKE0001 \nFAKEFAKEFAKE0002 \n-----END RSA PRIVATE KEY-----\n' > k.txt
$ fini --check --diff k.txt
# 1 行目は [line masked: potential private key] になるが、
# FAKEFAKEFAKE0001 / 0002 の行はそのまま表示される(コードからの予測)
```

# 影響範囲

- `--diff` を使うすべての経路(修正モード、`--check`、`--stdin --check --diff`)
- 修正方針: `BEGIN ... PRIVATE KEY` から対応する `END` 行までをまとめて伏せ字にする。`END` が無ければファイル末尾まで
````

---

## - [x] A-04 新規 issue: 秘密情報パターンの取りこぼし(B-04)

- 対象: 新規
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)
- 投稿前の再確認: 再現手順を実行し、検出されない行の一覧を実際の結果に合わせる。
- コマンド:
  ```bash
  gh issue create --title "[medium] Secret detection misses quoted keys (JSON), unquoted values (.env, YAML), OpenSSH/EC keys and newer GitHub token prefixes" --label enhancement --label security --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

正規表現を読む限り、次の一般的な書き方は検出されない。

| 例 | 検出されない理由 |
|---|---|
| `"password": "hunter2hunter2"`(JSON、Python の辞書) | キー名の直後に `=` か `:` を要求しており、閉じ引用符で不一致 |
| `password: hunter2hunter2`(YAML)、`API_KEY=abcd1234efgh5678`(.env) | 値に引用符が必須 |
| `-----BEGIN OPENSSH PRIVATE KEY-----`、`EC`、`DSA`、`ENCRYPTED` | `RSA` と種類なしのみ対応 |
| `gho_`、`ghu_`、`ghs_`、`ghr_`、`github_pat_` で始まるトークン | `ghp_` のみ対応 |
| `AKIA...` 単体 | `access_key_id =` などのラベルが必須 |
| `password = "dGVzdA=="` | 値の文字集合に `=` がない |

README は `.env` を秘密が入りやすい場所として挙げているが、`.env` の標準的な書き方(引用符なし)は検出できない。

**深刻度: medium**(検出は補助機能だが、`fini --check` を CI の関門として使う利用者に誤った安心を与える)

# 証拠

- `src/normalize/detect.rs:29-73`: パターン定義
- `src/normalize/detect.rs:51`: `(password|...)\s*[=:]\s*["'][a-zA-Z0-9_\-/+@#$%^&*!~.]{8,}["']`
- `src/normalize/detect.rs:456-723`: 検出の単体テストに上記の形はない

# 再現手順

上の表の各行を 1 行ずつ書いたファイルに対して `fini --check <file>` を実行し、`potential secret` が出ない行を確認する(未実行。値はすべてダミーにすること)。

# 影響範囲

- 秘密情報の検出全般と、同じパターンを使う diff の伏せ字
- 修正方針: キー側の引用符を許容する。引用符なしの値に対応する(誤検知とのバランスは要検討)。鍵ヘッダーとトークン接頭辞を追加する。README に検出できる形とできない形の一覧を載せる
````

---

## - [x] A-05 新規 issue: 常時有効の修正に除外手段がない(B-05)

- 対象: 新規
- 操作内容: 新規 issue(設計判断)
- 調査時点の状態: 該当 issue なし(2026-11-10)
- 投稿前の再確認: A-01 を先に起票し、本文の「#(A-01 の番号)」を実際の番号に置き換える。
- コマンド:
  ```bash
  gh issue create --title "design: always-on fixes (trailing whitespace, U+3000, LF) have no per-file opt-out and change meaningful content" --label needs-decision --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

行末の空白の除去、全角空白(U+3000)の半角化、改行コードの LF 化は、ファイルの種類に関係なく常に実行され、無効にする設定がない。次の内容は意味が変わる。

- Markdown の行末スペース 2 個(強制改行)
- TSV の行末のタブ(末尾の空の列)
- 日本語の文章中の全角空白(字下げ、氏名の区切り)

`.editorconfig` で `[*.md] trim_trailing_whitespace = false` のように種類別に指定していても読まれない(`[*]` だけを解釈する)。

逃げ道は `exclude` だけだが、ファイルを直接指定する経路(pre-commit、Claude Code フック)では `exclude` が効かない(#264)。

# 証拠

- `src/normalize/mod.rs:234-244`: 全角空白の変換と行末空白の除去は無条件
- `src/config/editorconfig.rs:25-67`: `[*]` セクションのみ解釈
- `src/config/editorconfig.rs:123`: `[*.md]` を無視することをテストが固定
- `README.md:120`: "always trims whitespace, always LF, always adds final newline"

# 再現手順

```console
$ printf 'line one  \nline two\n' > a.md      # 行末スペース 2 個 = 強制改行
$ fini a.md && od -c a.md                     # スペースが消える(コードからの予測)
```

# 影響範囲

- 修正モードを通るすべてのテキストファイル
- 選択肢:
  - A. ファイル種別ごとに修正を切り替える設定を追加する(例: glob ごとの上書き)
  - B. `.editorconfig` のセクション別指定を尊重する
  - C. 現状維持とし、README に影響と回避策(`exclude`、pre-commit 側の `exclude`)を明記する
````

---

## - [x] A-06 新規 issue: README の Claude Code フック例が動かない(D-01)

- 対象: 新規
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。#91(closed)の本文がこの設定例を引用している
- 投稿前の再確認: 実際に Claude Code で設定例を試し、フックがエラーになることを確かめる。公式ドキュメントの該当ページが変わっていないか確認する。
- コマンド:
  ```bash
  gh issue create --title "[high] README Claude Code hook example does not work: hook input arrives as JSON on stdin, not in TOOL_INPUT" --label bug --label documentation --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

README の「Claude Code Integration」の設定例は次のコマンドを使っている。

```json
"command": "fini \"$TOOL_INPUT.file_path\""
```

Claude Code はフックへの入力を環境変数ではなく標準入力の JSON で渡す。`$TOOL_INPUT` は未定義なので空文字になり、`fini` は `.file_path` という存在しないパスを受け取ってエラー終了する(終了コード 2 のはず)。README のとおりに設定しても、編集後の整形は行われない。

**深刻度: high**(「AI コーディングエージェント向け」という主用途の導線が動かない)

# 証拠

- `README.md:222-238`: 設定例
- 公式ドキュメント https://code.claude.com/docs/en/hooks.md : "For command hooks, input arrives on stdin." 編集対象のパスは `tool_input.file_path`
- 公式ガイド https://code.claude.com/docs/en/hooks-guide.md の整形の例:

```json
"command": "jq -r '.tool_input.file_path' | xargs npx prettier --write"
```

`$TOOL_INPUT` という変数や置換記法の記載は見つからなかった。

付随する問題: 対象ツールに `NotebookEdit` があるが、そのパスの項目名は `notebook_path`(Agent SDK の型定義による)。

# 再現手順

README の設定例を `.claude/settings.json` に入れ、Claude Code でファイルを編集する。フックが `.file_path` を引数に起動され、エラーになる(未実行。ドキュメントとシェルの展開規則からの予測)。

# 影響範囲

- README の設定例を使ったすべての利用者
- 修正方針: `jq -r '.tool_input.file_path' | xargs fini` の形に直す。空白を含むパスへの対処と、`jq` が必要な旨を書く。`NotebookEdit` は対象から外す
````

---

## - [x] A-07 新規 issue: Homebrew の定義が 0.5.0 で止まっている(C-01)

- 対象: 新規(関連: #2、#90、#241)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。`HomebrewFormula/fini.rb` はローカル `b42afdd` で `version "0.5.0"`
- 投稿前の再確認: リモートの main で `HomebrewFormula/fini.rb` がまだ 0.5.0 か、定義更新の PR が既に作られていないかを確認する。
- コマンド:
  ```bash
  gh issue create --title "[high] Homebrew formula is stuck at 0.5.0: the formula job was skipped for v0.6.0 and v0.7.0 and cannot be re-run for an existing tag" --label bug --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

最新リリースは v0.7.0(2026-10-13 公開)だが、`HomebrewFormula/fini.rb` は `version "0.5.0"` のまま。README の手順で Homebrew から導入すると 2 つ前の版になり、その後の修正(#89、#91、#156 など)が入らない。

**深刻度: high**(公式に案内している導入経路が古い版を配り続けている)

# 証拠

- `HomebrewFormula/fini.rb:4`: `version "0.5.0"`
- リリースワークフローの実行履歴: v0.4.0 以降すべて失敗
  - v0.6.0(実行 34747715013): `publish-crate` が "Publish to crates.io" で失敗し、`update-homebrew-formula` は未実行
  - v0.7.0(実行 38376637999): 同上

`publish-crate` は #240 で削除済み、定義更新は #257 で PR 作成方式に変更済み。ただしこの形での実行実績はない。

確認済みの前提: Actions による PR 作成は許可されている(`can_approve_pull_request_reviews: true`)。main のルールは PR 必須のみで、必須チェックはない。

# 再現手順

```console
$ brew tap tsukasaI/fini https://github.com/tsukasaI/fini
$ brew install tsukasaI/fini/fini && fini --version   # 0.5.0 になる(未実行。定義ファイルからの予測)
```

# 影響範囲

- Homebrew 経由の全利用者。pre-commit フックは PATH 上の `fini` を使うため、そちらにも波及する
- 対応案:
  1. v0.7.0 の定義を生成して PR にする(`gh release download v0.7.0` で成果物と `checksums.txt` を取得し、`bash scripts/update-homebrew-formula.sh 0.7.0 <dir>`)
  2. 定義更新ジョブに手動起動(`workflow_dispatch`、タグ指定)を追加し、既存タグでもやり直せるようにする
  3. 次のリリースで、PR 作成方式の初回実行を見届ける
````

---

## - [x] A-08 新規 issue: README の GitHub Action の例が v0.4.0 のまま(D-02)

- 対象: 新規(再発元: #36)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。#36 は closed(最終更新 2026-07-17T13:30:42Z)
- 投稿前の再確認: リモートの main の README がまだ `@v0.4.0` を指しているか確認する。
- コマンド:
  ```bash
  gh issue create --title "[medium] README Action examples pin v0.4.0, whose action.yaml predates the fixes for #96 and #97" --label documentation --label security --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

README の GitHub Action の例は `tsukasaI/fini@v0.4.0` を指している。v0.4.0 時点の `action.yaml` は次の状態で、のちに #96 と #97 で修正された問題を含む。

- `verify-attestation` の既定値が `'false'`
- 最新版の問い合わせが未認証
- 実行ファイルを利用者のリポジトリ直下に展開し、そのディレクトリを PATH に追加

README のコメントは「既定で true」と説明しており、例の版と食い違う。オプションの例も `version: 'v0.3.0'` と `verify-attestation: 'false'` で、最も弱い組み合わせを示している。

**深刻度: medium**(README をコピーした利用者が、修正前の Action を使い続ける)

# 証拠

- `README.md:243`、`README.md:255`: `uses: tsukasaI/fini@v0.4.0`
- `README.md:259-265`: `version: 'v0.3.0'`、`verify-attestation: 'false'`、「defaults to true」のコメント
- `git show v0.4.0:action.yaml`: `default: 'false'`、`curl -fsSL https://api.github.com/...`(認証ヘッダーなし)、`echo "$PWD" >> "$GITHUB_PATH"`

# 再現手順

```console
$ git show v0.4.0:action.yaml | grep -n -e "default: 'false'" -e 'GITHUB_PATH'
```

# 影響範囲

- README の例をそのまま使う Action 利用者
- 修正方針: 例を最新のタグに更新する。リリース手順の更新対象に README の Action の例を加える。オプションの例は既定値のままにし、`verify-attestation: 'false'` は古い版を固定する場合の注記に留める
````

---

## - [x] A-09 #241 へのコメント: SKILL.md の完了条件が未達(D-03)

- 対象: issue #241
- 操作内容: コメント(必要なら再オープン)
- 調査時点の状態: closed(completed)、最終更新 2026-10-19T02:48:36Z
- 投稿前の再確認: リモートの main で `.claude/skills/release-playbook/SKILL.md` に "verify the match yourself" がまだ残っているか、#241 に新しいコメントが付いていないかを確認する。
- コマンド:
  ```bash
  gh issue comment 241 --body-file <本文ファイル>
  # 再オープンする場合
  gh issue reopen 241
  ```
- 投稿本文:

````markdown
完了条件のうち SKILL.md に関する 2 点が、`b42afdd` 時点で未達のままです。

**事実**
- `.claude/skills/release-playbook/SKILL.md:44-48` に "No job checks the tag against the `Cargo.toml` version any more ... so verify the match yourself before pushing." が残っています。トリアージの完了条件は「SKILL.md no longer contains "verify the match yourself"」でした。照合ステップ自体は #248 で入っています(`.github/workflows/release.yaml:35-49`)。
- リリース後の更新対象の表(`SKILL.md:84-91`)に、README のバージョン記載(`cargo install ... --tag`、pre-commit の `rev`)がありません。トリアージの Item 3 は「add the README pins to the release-playbook propagation steps」でした。

**その後に古くなった箇所**
- `SKILL.md:65-68` と `:88` は Homebrew の定義を「pushes it to `main`」と説明していますが、#257 で PR 作成方式に変わっています。
- `SKILL.md:100-101` は `gh issue view 2` で issue #2 がまだ open かを確認する手順になっていますが、#2 は 2026-07-17 に閉じています。

**提案**
上の 4 箇所を直す PR を出すか、この issue を再オープンして追跡するのがよいと考えます。トリアージに「SKILL.md の編集は Claude のサンドボックス外でコミットする必要がある」とあるので、手作業のコミットが必要です。
````

---

## - [x] A-10 新規 issue: リリースと CI の設定(C-02、C-03、C-04、C-05、C-13、R-01)

- 対象: 新規(関連: #241、PR #254)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)
- 投稿前の再確認: `ubuntu-latest` の移行状況と、PR #254 がマージ済みかを確認し、本文を合わせる。
- コマンド:
  ```bash
  gh issue create --title "Release and CI hardening: pin build runners, gate releases on tests, run CI on bot PRs" --label enhancement --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

リリース前の点検で見つかった、リリースワークフローと CI の設定に関する項目をまとめる。

# 項目

1. **[medium] Linux 向けバイナリのビルド環境が固定されていない**
   `.github/workflows/release.yaml:19-22` は `ubuntu-latest`。v0.7.0 の実行ログに「2026-10-19 から Ubuntu 26 に移行」の注記があった。次のリリースは新しい glibc にリンクされ、古いランナー(Ubuntu 22.04 / 24.04、自前のランナー)で起動できなくなる可能性がある(未検証)。GitHub Action は既定で最新版を取得する。
   案: 最も古いサポート対象(例: `ubuntu-22.04`)に固定する、または musl で静的リンクする。

2. **[medium] リリースがテストの成否と無関係に進む**
   タグの push でビルドと公開が始まり、テストは実行されない。タグのコミットが main にあるか、CI が成功しているかも確認しない。`cargo build --release` に `--locked` がない(`release.yaml:59`)。
   案: ビルド前に `cargo test --locked` を実行する。タグのコミットが main に含まれることを確認する。ビルドに `--locked` を付ける。

3. **[medium] ボットが作る PR に CI が走らない**
   #155 のチェックは Dependabot 設定の検証 1 件のみ。ワークフロー既定のトークンで作った PR は他のワークフローを起動しないため。#257 で導入した Homebrew 定義の PR も同じ扱いになる。main に必須チェックはない。CI に Nix のビルドもない。
   案: これらの PR でも CI を起動する手段を用意する。CI に `nix build` を追加する。

4. **[low] CI の検査範囲**
   - clippy は `cargo clippy -- -D warnings` で、テストコードを検査しない(`ci.yaml:34`、`.pre-commit-config.yaml:17`)。トリアージの検証手順は `--all-targets` 付き。現状は `--all-targets` でも警告なし
   - テストは Ubuntu のみ。macOS 専用テスト(`test_issue_100`)は CI で一度も実行されない
   - 依存の脆弱性検査は push と PR のときだけで、定期実行がない

5. **[low] プレリリースのタグが通常のリリースになる**
   `v1.0.0-rc.1` のようなタグは形式検証(`release.yaml:136`)を通り、リリースはプレリリース扱いにならない。Action の既定と Homebrew の定義がプレリリースを指す。
   案: `-` を含むタグでは `prerelease: true` とし、定義更新を行わない。

# 影響範囲

`.github/workflows/release.yaml`、`.github/workflows/ci.yaml`、`.pre-commit-config.yaml`
````

---

## - [x] A-11 新規 issue: 検証が弱いテスト(T-01 から T-06)

- 対象: 新規(関連: #172、#190)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。#172 は closed(最終更新 2026-10-13T02:20:00Z)
- 投稿前の再確認: リモートの main(#258 以降)で行番号がずれていないか確認する。
- コマンド:
  ```bash
  gh issue create --title "test: tests that cannot fail or do not exercise what their name claims (git-root stop, CLI precedence, flag wiring)" --label enhancement --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

通過しているが、名前やコメントが主張する内容を検証していないテストの一覧。行番号は `b42afdd` 時点。

# 項目

1. **[medium] `src/config/file.rs:137` `test_find_config_stops_at_git_root`**
   `.git` とサブディレクトリを作るだけで、`fini.toml` をどこにも置いていない。探索を止める処理(`file.rs:70-72`)を削除しても結果は `None` のまま。
   案: `.git` より上のディレクトリに `fini.toml` を置き、拾わないことを確認する。

2. **[low] `tests/integration.rs:399` `test_cli_overrides_config_file`**
   CLI フラグを渡していない(コメントも "Run without CLI override")。設定ファイルの値が効くことしか確認していない。

3. **[low] 終了コードを「0 以外」としか確認しない**
   `tests/integration.rs:698`(`--strict-debug`)と `:1985`(`--stdin --check`)。フラグを削除して引数エラー(終了コード 2)になっても通る。
   案: `code() == Some(1)` と出力の文言を確認する。

4. **[low] CLI フラグの配線が未検証**
   `--max-blank-lines`(実際に空行が減る入力がない)、`--no-color`(`src/colors.rs:55` の分岐)、`--no-detect-fixmes`、`--no-detect-secrets`、`--no-detect-debug`、`--keep-leading-blanks`、CLI の `--fix-code-blocks`。`src/main.rs:338-355` の対応付けを取り違えても検出できない。

5. **[low] 書き込み直前の安全確認が CI で未検証**
   `tests/integration.rs:927` `test_issue_100` は macOS 専用で、CI は Ubuntu のみ。`src/lib.rs:426-437` の「読み取り後に変更された」「シンボリックリンクに差し替えられた」場合の拒否を固定するテストはない。

6. **[low] その他**
   - `src/normalize/mod.rs:1162` `test_code_fence_with_numbers_not_removed` は名前と逆に「除去される」ことを確認している
   - `tests/integration.rs:2562` `test_issue_84` は `:354` と同一内容
   - `tests/integration.rs:2610` の `header_lines == 1` は失敗しえない(後続の検証が実質)
   - `tests/integration.rs:1610` の `contains(":1")` は `:10` にも一致する

# 影響範囲

`src/config/file.rs`、`tests/integration.rs`、`src/normalize/mod.rs`、`src/colors.rs`(テストのみ)
````

---

## - [x] A-12 新規 issue: Low の指摘まとめ(B-06 から B-12、C-06、C-09、D-04 から D-08)

- 対象: 新規(関連: #217、#75、#35、#78、#79、#90)
- 操作内容: 新規 issue
- 調査時点の状態: 該当 issue なし(2026-11-10)。関連 issue はすべて closed
- 投稿前の再確認: 1 件ずつ起票する運用(過去の `[low]` issue の形式)に合わせるなら、項目ごとに分割する。
- コマンド:
  ```bash
  gh issue create --title "[low] Pre-release audit: low-severity behavior and documentation findings (2026-11)" --body-file <本文ファイル>
  ```
- 投稿本文:

````markdown
# 概要

リリース前の点検で見つかった Low の項目。挙動はコード読解によるもので、実行での再現はしていない。行番号は `b42afdd` 時点。

# 挙動

1. **`fini:ignore` は `*/` と `-->` 以外の終端や説明文が続くと黙って無効になる**(`src/normalize/ignore.rs:111-134`)。`{# ... fini:ignore #}`、`(* ... fini:ignore *)`、`# fini:ignore todo -- 理由` は未知の種類として扱われ、何も抑制されない。README は "Works with any comment syntax"。
2. **TODO / FIXME の判定に語頭の境界がない**(`src/normalize/detect.rs:87-103`)。スペイン語などの `todo ` や `método ` が TODO として報告される。
3. **走査中のシンボリックリンクは件数にも一覧にも出ない**(`src/walker.rs:115` で通常ファイル以外を捨てる)。`README.md:214-215` は「要約に数え、`--verbose` で一覧できる」としている。
4. **`--quiet` の修正モードは書き換えていないファイル名も出力する**(`src/output.rs:287`)。ヘルプは "Output only modified file names"(`src/main.rs:51`)。README は 2 通りの説明(`:55`、`:66`)。
5. **Action の `version` に `0.7.0` 形式を渡すと 404**。`action.yaml:57-64` は受け付けるが、`:84` の URL にそのまま使う。タグは `v` 付き。
6. **`--stdin` の修正モードは検出結果を出さない**(`src/main.rs:235-290`)。同時に渡したパス、`--diff`(`--check` なし)、`--exclude`、`--hidden` は黙って無視される。
7. **Homebrew 更新スクリプトは一覧に無いファイルを検証せずに使う**(`scripts/update-homebrew-formula.sh:38-47`)。`checksums.txt` から 1 件欠けていても検証が通る。

# 設定

8. **バージョン番号の手作業同期**(`Cargo.toml:3`、`flake.nix:18`、`README.md:14`、`:39`、`:46`)。一致を確かめる仕組みがない。
9. **pre-commit フックは実行ファイルの版を固定しない**。`.pre-commit-hooks.yaml` は `language: system` で、`rev` が固定するのは定義だけ。

# ドキュメント

10. **README の設定例は `[normalize]` 11 項目中 4 項目のみ**(`README.md:93-106`)。検出対象の一覧もない。
11. **書き込み時に所有者と拡張属性が失われる点が README にない**。`src/lib.rs:468-469` のコメントは「文書化済みの制限」としている。
12. **`.ignore` の説明と追跡済みファイルの扱い**。`README.md:211-212` は「どこでも尊重」、`src/walker.rs:136-138` は追跡済みファイルを再度対象に加える。
13. **README にビルドとテストのコマンドがない**(`README.md:285-300`)。
````

---

## - [x] A-13 クローズ提案: 削除済みディレクトリ向けの依存更新 PR(C-07)

- 対象: PR #153、#168、#197
- 操作内容: クローズ(コメント付き)
- 調査時点の状態:
  - #153 open、最終更新 2026-09-22T22:43:38Z
  - #168 open、最終更新 2026-10-06T22:44:32Z
  - #197 open、最終更新 2026-10-14T18:53:22Z
- 投稿前の再確認: 3 件がまだ open か、`editors/vscode` がリモートの main に復活していないかを確認する。
- 理由: 対象の `editors/vscode` は #206 で削除済みで、`.github/dependabot.yml` に npm の設定もない。PR #254 の本文も同じ結論を述べている。
- コマンド:
  ```bash
  gh pr close 153 --comment "Closing: editors/vscode was removed in #206, so this update no longer applies."
  gh pr close 168 --comment "Closing: editors/vscode was removed in #206, so this update no longer applies."
  gh pr close 197 --comment "Closing: editors/vscode was removed in #206, so this update no longer applies."
  ```
- 投稿本文(3 件共通):

````markdown
Closing: editors/vscode was removed in #206, so this update no longer applies.
````

---

## 提案しない操作(参考)

| 対象 | 理由 |
|---|---|
| #243 / PR #255、#249 / PR #253、#246 | 選択肢と推奨が既に書かれており、今回の調査で新しい根拠はない |
| PR #254 | マージの判断のみ。本文の内容と今回の調査結果に食い違いはない。マージ後に #149、#196、#250、#251、#252、#155 を閉じる手順は #254 の本文に記載済み |
| PR #256 | #254 より後に作られた更新で、#254 には含まれていない。個別に判断 |
