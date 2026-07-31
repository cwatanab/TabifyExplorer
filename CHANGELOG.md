# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.6] - 2026-07-31

### Fixed
- `enable_log = true` 指定時にアプリケーション起動時のログファイル生成および初期化がスキップされる問題を修正。
- zip などの圧縮フォルダをダブルクリックで開いた際、新規ウィンドウ初期画面（「ホーム」）でパス判定が固定化されてしまう現象を修正。
- コマンドライン引数の `/select,` プレフィックス解析を追加し、PEB から 0ms でのパス超即時パース精度を向上。

### Documentation
- README および仕様書に zip 統合対応、PEB パス解析、ログ設定に関する仕様を追記・更新。

[0.3.6]: https://github.com/cwatanab/TabifyExplorer/compare/v0.3.5...v0.3.6
