# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.8] - 2026-08-04

### Fixed
- タブのドラッグ操作直後に新規エクスプローラーウィンドウが誤って既存タブへ再統合されるケースを修正。
- フォルダをダブルクリックして開いた際、PEB 由来の候補パスを即確定値として扱わず COM で取得したウィンドウ固有パスを優先するよう改善し、タブ統合が漏れるケースを修正。
- 既存タブと同一パスを開いた通常操作をドラッグアウト分離として誤判定する条件を修正。

## [0.3.7] - 2026-07-31

### Fixed
- バックグラウンド COM `Navigate2` 遷移における RPC NULL 参照エラー (`0x800706F4`) を修正し、キー操作非依存の安定なフォルダ遷移を実現。
- 新規追加タブの `IWebBrowser2` インスタンス精密切出し（COM ポインタ比較）を実装し、既存タブが誤って上書きされる不具合を解消。
- タブ作成・アクティブ化・フォルダ遷移処理間の不要なスリープ待機（`thread::sleep`）を完全撤去（0ms化）し、「ホーム」画面表示の切り替わりチラつきを追放。

### Documentation
- README および仕様書にネイティブ COM 遷移、新規タブ識別、0ms 完全スリープレス設計に関する仕様を最新化。

[0.3.8]: https://github.com/cwatanab/TabifyExplorer/compare/v0.3.7...v0.3.8
[0.3.7]: https://github.com/cwatanab/TabifyExplorer/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/cwatanab/TabifyExplorer/compare/v0.3.5...v0.3.6
