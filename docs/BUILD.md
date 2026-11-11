# 빌드하고 고치기

[README](../README.md)는 받아 쓰는 사람을 위한 글입니다. 여기에는 소스를 받아 빌드하고 고치는 데 필요한 것을 적습니다.

## 빌드

Windows 10 이상(64비트), Rust stable, Windows SDK가 필요합니다. 설정 창의 틀을 exe에 넣을 때 SDK의 `rc.exe`를 씁니다. Visual Studio Build Tools에서 "C++를 사용한 데스크톱 개발"을 고르면 함께 설치됩니다.

```
cargo build --release
```

실행 파일은 `target/release/DesktopFlowerpin.exe`에 만들어집니다.

### 그림

꽃핀 그림은 이 저장소에 없습니다. README에 쓰는 화면 그림 셋(`docs/images`)만 있습니다. 이 저장소만 받아 빌드하면 대체 그림 팩(`placeholder/flowerpin.dfpk`)이 들어가고, 꽃핀 대신 눈이 둘 달린 둥근 기둥이 나옵니다. 코드를 고치고 확인하는 데는 이것으로 충분합니다.

진짜 그림은 공식 배포판에만 들어 있습니다. 그림 팩은 암호화되어 있고, 그 키도 이 저장소에 없습니다.

| 환경 변수 | 뜻 |
|---|---|
| `DFP_PACK_DIR` | 진짜 그림 팩(`flowerpin.dfpk`)이 있는 폴더. 정하지 않으면 저장소 옆의 `../local`을 봅니다 |
| `DFP_PACK_KEY` | 진짜 그림 팩의 키(16진수 64글자). 없으면 팩 폴더의 `pack.key`를 읽습니다 |
| `DFP_RELEASE` | `1`이면 공식 빌드입니다. 개발 키나 대체 그림 팩이면 빌드가 실패합니다 |

### 확인

```
cargo fmt --check
cargo clippy --all-targets
cargo test
```

`cargo test`에는 이 저장소에 그림, 그림 팩, 키가 들어가지 않았는지 보는 검사가 들어 있습니다. README의 화면 그림을 바꾸거나 더하면 `.gitignore`와 `crates/app/tests/repo_guard.rs`의 목록도 함께 고쳐야 합니다.

위의 셋은 올리기 전에 Windows에서 직접 돌려 주세요. Windows의 창과 글꼴을 쓰는 테스트가 많아 다른 곳에서는 돌릴 수 없습니다. 올릴 때마다 GitHub Actions(`.github/workflows/ci.yml`)는 이 저장소만으로 Windows용 실행 파일이 빌드되는지만 봅니다. 리눅스에서 MinGW로 빌드합니다.

### 공식 배포 파일

Releases에 올리는 파일은 진짜 그림 팩과 그 키가 있는 PC에서 `scripts/release.ps1`로 만듭니다. GitHub Actions에서는 만들지 않습니다. 그림과 키가 거기에 없기 때문입니다.

```
scripts/release.ps1          # 형식, clippy, 테스트, 공식 빌드, 압축 파일과 SHA-256 (target/dist)
scripts/release.ps1 -Draft   # 위에 더해 GitHub에 릴리스 초안을 만듦
```

- 압축 파일에는 실행 파일, 안내 글(`packaging/읽어 주세요.txt`), 라이선스 셋이 들어갑니다.
- 실행 파일은 DLL을 Windows의 시스템 폴더에서만 불러옵니다. 실행 파일 옆에 같은 이름의 DLL이 놓여 있어도 불러오지 않게 하려는 것입니다. 링크할 때 넣는 설정(`crates/app/build.rs`)과 켠 직후에 부르는 함수(`win::search_system_folder_only`)가 맡고, 스크립트가 만든 실행 파일에 그 설정이 들어 있는지 봅니다. 새 DLL의 함수를 쓰게 되면 실행 파일 옆에 그 이름의 빈 파일을 두고도 켜지는지 확인해 주세요.
- 공식 빌드는 명령줄 인자를 받지 않습니다. 만드는 동안 확인하는 데 쓰는 인자(`crates/app/src/dev.rs`)는 파일과 레지스트리를 쓸 수 있어서, 공식 빌드가 아닌 빌드에서만 듣습니다.
- 그림과 키가 공개되어 있지 않아서, 다른 사람이 똑같이 다시 빌드해 견줘 볼 수는 없습니다. 릴리스 글에 빌드한 커밋과 SHA-256을 적습니다.

## 폴더

| 폴더 | 내용 |
|---|---|
| `crates/app` | 캐릭터 실행 파일 |
| `crates/pack` | 그림 팩 형식. 읽기와 쓰기, 암호화, 키 다루기 |
| `placeholder` | 대체 그림 팩 |
| `docs` | 이 글과, README에 쓰는 화면 그림(`docs/images`) |
| `packaging` | 배포 파일에 넣는 안내 글과 릴리스 글의 틀 |
| `scripts` | 공식 배포 파일을 만드는 스크립트 |
