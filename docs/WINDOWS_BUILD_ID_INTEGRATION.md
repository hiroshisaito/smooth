# Windows 追従指示: Build-id UI 表示対応

このドキュメントは、Mac 側で実装された Build-id UI(commit: TBD、branch `feature/build-id-display`)を Windows 機側に取り込むための手順をまとめたものです。通常の開発フローに従って追従してください。

## TL;DR

**Windows 固有の変更は必要ありません**。master へマージされた時点で `git pull` → VS で開き直して Clean → Rebuild すれば、新しい `Build` パラメータが Effect Controls に表示されるはずです。

## 変更内容の概要

| ファイル | 変更 | Windows 影響 |
|---|---|---|
| `rust/smooth_core/build.rs`(新規) | cargo build.rs が `git rev-parse --short HEAD` + dirty 判定で `SMOOTH_CORE_GIT_SHA` を生成 | Windows 側も cargo build 時に同じ build.rs が走る。**Git for Windows が PATH にあること前提**(`cargo` / `cl.exe` と同じ前提条件) |
| `rust/smooth_core/src/lib.rs` | `smooth_core_build_id()` FFI 追加 | Rust 側のみ。Win MSBuild の PreBuildEvent が既に `build-windows.bat` → cargo を呼ぶので自動ビルド |
| `rust/smooth_core/include/smooth_core_ffi.h` | `smooth_core_build_id` 宣言追加 | `win.vcxproj` の include path に既に入っているため追加対応不要 |
| `Effect.cpp` | `PARAM_BUILD_INFO` enum 追加 + `PF_Param_BUTTON` + About の return_msg 更新 + `my_version` bump | 共有ソース。Win ビルドも自動反映 |
| `workbench_history.md` | 記録 | ドキュメントのみ |

**変更しないもの**: `win/win.vcxproj` / `win/Pipl.r` / `win/BUILD_WINDOWS.md` / `rust/smooth_core/build-windows.bat` / `rust/smooth_core/.cargo/config.toml`(Windows 特有の設定には触らない)

## 追従手順(通常の開発フロー)

### 1. `master` を sync

PR がマージされた後、Windows 機で:

```cmd
cd /d D:\GitHub\smooth
git checkout master
git pull --ff-only origin master
```

### 2. 事前準備確認

以下が前回の Phase 2-D 対応時点で既に整っているはずですが、念のため:

```cmd
where git
where cargo
rustup target list --installed
```

`git.exe`(Git for Windows), `cargo.exe`, `x86_64-pc-windows-msvc` target がそれぞれ見えれば OK。

### 3. クリーンビルド(**重要** — 偽成功を避ける)

```cmd
rem VS Developer Command Prompt for VS2022
cd /d D:\GitHub\smooth\win
rmdir /s /q Release 2>nul
rem Visual Studio で win.sln を開いて "Build → Rebuild Solution" (Release|x64)
rem もしくは:
msbuild win.sln /t:Rebuild /p:Configuration=Release /p:Platform=x64
```

**重要**: Phase 2-D で一度やらかした incremental-cache による偽成功を避けるため、必ず Release ディレクトリを削除してからリビルドしてください。

### 4. 成功検証(3 段階)

#### 4a. バイナリサイズ確認

```cmd
dir win\Release\x64\smooth.aex
```

Phase 2-D 時点の `393,216 バイト` 前後 ± 数 KB のはずです。大きく外れたら何かがおかしい。

#### 4b. FFI シンボル確認(**新項目、重要**)

**`.aex` ではなく Rust staticlib 側を見る**こと。Release + LTO
(`WholeProgramOptimization=true`、現 vcxproj のデフォルト)だと、
`.aex` 側では FFI 呼び出しが caller に inline 展開され、PE の
シンボルテーブルには 6 FFI が残らない(返値 0 件)。FFI 実在の証明は
`.lib` 側の External symbol で行い、`.aex` 側は埋め込み文字列(4c)と
unmangled `EntryPointFunc` export(§5)で補強する。

```cmd
dumpbin /symbols rust\smooth_core\target\x86_64-pc-windows-msvc\release\smooth_core.lib ^
  | findstr /i smooth_core_
```

**期待出力(6 個の External symbol)**:
```
  smooth_core_build_id          ← 新規(Build ID FFI)
  smooth_core_preprocess_u16
  smooth_core_preprocess_u8
  smooth_core_process_row_range_u16
  smooth_core_process_row_range_u8
  smooth_core_version
```

いずれかが `External` としてリストされない場合、Rust 側のビルドが
古いキャッシュから来ている。`rd /s /q rust\smooth_core\target` で
手動削除してから再ビルド。

参考: `.aex` 側でも確認したい場合は以下で埋め込み Rust 関数名の
文字列(strings 相当)が検出できる(LTO で消えても `BUILD_ID` の
生データと一緒にラベルが残る場合あり):

```cmd
findstr /i smooth_core_ win\Release\x64\smooth.aex
```

ただしこれは**補助的**で、`.lib` 側の External シンボル確認が正道。

#### 4c. 埋め込み文字列確認

```cmd
rem Windows 版 strings 相当(findstr はバイナリにも使える)
findstr /c:"0.1.0+" win\Release\x64\smooth.aex
```

`0.1.0+<7文字のSHA>` 形式の文字列が見つかれば、ビルド識別子が正しく埋め込まれている証拠。

### 5. AE 2025 実機確認

#### 5a. インストール

```cmd
copy /y win\Release\x64\smooth.aex "D:\Program Files\Adobe After Effects 2025\Support Files\Plug-ins\Effects\smooth.aex"
```

#### 5b. UI 目視確認

1. AE 起動、適当なコンポジションに smooth エフェクト適用
2. Effect Controls パネルに以下の 4 行が見えるはず:
   - `transparent` (checkbox)
   - `range` (slider)
   - `line weight` (slider)
   - **`Build` (button、キャプション `0.1.0+<sha>[+dirty]`)** ← 新規
3. エフェクト名右クリック → `Effect Info` → About ダイアログに:
   ```
   smooth, v1.5.0
   rust_core 0.1.0+<sha>[+dirty]  ffi=0x00020003
   ```
   と表示されるはず

#### 5c. SHA マッチ確認

表示される SHA が以下と一致することを確認:

```cmd
git rev-parse --short HEAD
```

**不一致の場合は古いビルドがロードされている** → AE 再起動 or install 先の .aex を上書きし直し。

### 6. 配布 zip 更新(必要なら)

```cmd
powershell Compress-Archive -Path win\Release\x64\smooth.aex -DestinationPath win\release\smooth.Win.1.5.0.AE2025.x64.zip -Force
certutil -hashfile win\release\smooth.Win.1.5.0.AE2025.x64.zip SHA256
certutil -hashfile win\Release\x64\smooth.aex SHA256
```

新しい SHA256 を workbench_history.md の Phase 2-D git-state テーブルに追記(Phase 2-D クローズ時の SHA `24FEFCFA...` から更新される想定)。

## トラブルシュート

| 症状 | 原因 | 対処 |
|---|---|---|
| `smooth_core_build_id` unresolved external | Rust lib が古いキャッシュから来ている | `rd /s /q rust\smooth_core\target` → 再ビルド |
| `Build` ボタンのキャプションが `0.1.0+unknown` | `git` が PATH に無い or Rust build が `.git/` にアクセスできない | Git for Windows を PATH に追加、もしくは VS を管理者権限で起動 |
| `Build` ボタンに `+dirty` が付く | 作業ツリーに未 commit の変更あり | 配布前に commit して再ビルド |
| About ダイアログが古い書式(`rust_core ffi=0x00020002` のみ) | Effect.cpp のリビルドがキャッシュヒット | `rd /s /q win\Release` → Rebuild |
| AE 起動時に "プラグインが読み込めない" | `my_version` bump による古い project 読み込み問題 | 既存 AEP を開いた時のエフェクトパラメータが一部リセットされる場合あり(想定内、migration)|

## Phase 2-D 偽成功再発防止チェックリスト

### 最短の 1 段確認(日常運用)

ビルドのたびにこれだけはやる:

- [ ] AE で Effect Controls の `Build:` キャプションが `git rev-parse --short HEAD` と一致

一致していれば、.aex は確実に現 HEAD から作られ、Rust FFI(`smooth_core_build_id()`)も実行されている(ここで FFI が呼ばれていなければキャプション自体が出ない)。

### フル 3 段確認(配布前・主要変更時)

- [ ] `findstr /c:"0.1.0+" win\Release\x64\smooth.aex` で正しい SHA 文字列が埋まっていることを見る(§4c)
- [ ] `dumpbin /symbols smooth_core.lib` で 6 FFI を External 確認(§4b、**.aex ではなく .lib に対して**実行)
- [ ] `dumpbin /exports smooth.aex | findstr EntryPoint` で unmangled `EntryPointFunc` を確認(§ 末尾)
- [ ] AE 目視の 1 段確認も満たす

これが全部通らないまま「動きました OK」と報告するのは NG(Phase 2-D 前半の偽成功と同じ状態)。

## パラメータ追加時の注意: `my_version` と `AE_Effect_Version` の同期

Mac 側の初回インストールで AE が `effect "smooth" has version mismatch.
Code version is 2.0 and PiPL version is 2.0. (100200)` を表示して effect を
拒否する事故があった。

- 原因: `Effect.cpp::GlobalSetup` の `out_data->my_version` を
  `PF_VERSION(2,0,0,0,0)` → `PF_VERSION(2,0,0,1,0)` にだけ bump し、
  [Pipl.r](../Pipl.r) の `AE_Effect_Version` を更新し忘れていた
- 修正: `Pipl.r` の `AE_Effect_Version` を同じ数値(十進 1049088 =
  `0x100200` = `PF_VERSION(2,0,0,1,0)`)に揃える

**ルール**: 今後パラメータの追加・削除などで `my_version` を bump する際は
`Pipl.r::AE_Effect_Version` も**必ず同じ値で同期**。PiPL は `.rc` を経由して
コンパイルされるため、Windows 側は VS で普通にリビルドすれば追従する
(PiPLTool が Pipl.r から `win/Pipl.rc` を再生成 → `win.aex` に焼き込み)。

この同期忘れは `/MT` 混在や packing 違いと並ぶ典型的なハマりどころ。
Windows 側で version bump を触る機会があれば、`Effect.cpp::my_version` と
`Pipl.r::AE_Effect_Version` をセットで確認してください。

## Build ボタンと `PF_ParamFlag_SUPERVISE`

Build ボタンをクリックしたら About ダイアログが開く動作は、`PF_Param_BUTTON`
の `def.flags` に `PF_ParamFlag_SUPERVISE` を立て、かつ `EntryPointFunc` が
6 番目の `void *extra` 引数を受けて `PF_Cmd_USER_CHANGED_PARAM` を処理して
初めて成立します。どちらか欠けるとクリックは no-op になります。

Mac 側で以下の流れで修正済(commit `024d084` に amend 統合):
1. `Effect.cpp` ParamsSetup の Build param flags に `PF_ParamFlag_SUPERVISE` 追加
2. `EntryPointFunc` を 5 引数 → 6 引数(`void *extra` 追加)
3. `case PF_Cmd_USER_CHANGED_PARAM` を追加し、`extra` を `PF_UserChangedParamExtra*` にキャストして `param_index == PARAM_BUILD_INFO` なら `About()` を呼ぶ

Windows 側は `Effect.cpp` が共有ソースなので自動追従します。ビルド後に AE で
`Build: 0.1.0+<sha>` をクリック → About ダイアログが `rust_core 0.1.0+<sha>
[+dirty]  ffi=0x00020003` を含む形で開くことを確認してください。

## `Effect.h` と `Effect.cpp` のシグネチャ一致ルール

Mac 側で Missing Effect 事故が 1 件発生しました。原因:
`Effect.h` の `EntryPointFunc` 宣言が **5 引数**(古いまま)、
`Effect.cpp` の定義が **6 引数**(`void *extra` 追加) になっていた。
両者のシグネチャが不一致だと C++ 側で `extern "C"` が適用されず、
symbol が **C++ マングル名**でエクスポートされるため AE は
`Couldn't find main entry point for smooth.plugin` で
プラグインを読み込めなくなる。

### 自己診断コマンド(Windows)

```cmd
dumpbin /exports win\Release\x64\smooth.aex | findstr EntryPoint
```

期待: `EntryPointFunc` がマングルなしで出力される(先頭に `?` や
`_Z` が付いていない)。もし以下のようにマングル名が出ているなら
`Effect.h` と `Effect.cpp` の signature が不一致:
```
?EntryPointFunc@@YAJW4PF_Cmd@@PEAUPF_InData@@...
```

### Mac 側の確認コマンド(参考)

```bash
nm Mac/build/Release/smooth.plugin/Contents/MacOS/smooth | grep EntryPoint
# 期待: "T _EntryPointFunc" (leading _ は Mach-O 慣例)
```

`__Z14EntryPointFunc...` のように Itanium ABI マングル名が出ていたら
ビルドは失敗した状態。`Effect.h` と `Effect.cpp` のシグネチャを確認。

## コミット済みファイル一覧(参考)

```
rust/smooth_core/build.rs                     新規
rust/smooth_core/src/lib.rs                   + smooth_core_build_id() / version bump
rust/smooth_core/include/smooth_core_ffi.h    + build_id 宣言 + doc
Effect.cpp                                     + PARAM_BUILD_INFO + button + About 更新 + my_version bump
workbench_history.md                           + "Build-id UI 追加" セクション
docs/WINDOWS_BUILD_ID_INTEGRATION.md           本ドキュメント(新規)
```

## 連絡事項

Mac 側はクリーンビルド + 回帰テスト + 動作確認すべてパスしています(`SMOOTH_PARALLEL=0/1` 両方で 14/14 IDENTICAL 維持、合成 white_option 6/6 OK)。Windows 側で上記手順を踏んでも同等の結果になるはず。

**Phase 2-D 偽成功の再発を防ぐ観点から**: Build ボタンのキャプションで SHA が目に見えるため、今後は「AE で動いた」だけでなく「SHA が合っている」も確認基準に入れる運用に移行します。
