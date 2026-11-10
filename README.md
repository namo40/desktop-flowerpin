# DesktopFlowerpin

꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터입니다. 꽃핀 공지에 따라 만든 무료 팬게임이고, 판매하지 않습니다.

**아직 만드는 중입니다.** 지금은 시제품 단계라 배포판이 없습니다.

## 이 프로그램은

- 하루 종일 켜 둬도 부담이 없게, 메모리와 CPU를 적게 쓰는 것을 먼저 챙깁니다.
- 관리자 권한을 쓰지 않습니다. 설치 없이 exe 하나로 실행합니다.
- 설정은 `%APPDATA%\DesktopFlowerpin` 폴더에만 저장합니다.
- 꽃핀이 다른 창 위에 올라설 수 있게, 화면에 보이는 창들의 자리와 앞뒤 순서를 읽습니다. 창의 제목이나 내용은 읽지 않습니다.
- 인터넷은 꽃핀 방송 상태를 확인할 때만 쓰고, 설정에서 끌 수 있습니다. (아직 만들지 않은 기능입니다.)

## 빌드

Windows 10 이상(64비트), Rust stable, Windows SDK가 필요합니다. 설정 창의 틀을 exe에 넣을 때 SDK의 `rc.exe`를 씁니다. Visual Studio Build Tools에서 "C++를 사용한 데스크톱 개발"을 고르면 함께 설치됩니다.

```
cargo build --release
```

실행 파일은 `target/release/DesktopFlowerpin.exe`에 만들어집니다.

### 그림

꽃핀 그림은 이 저장소에 없습니다. 이 저장소만 받아 빌드하면 대체 그림 팩(`placeholder/flowerpin.dfpk`)이 들어가고, 꽃핀 대신 눈이 둘 달린 둥근 기둥이 나옵니다. 코드를 고치고 확인하는 데는 이것으로 충분합니다.

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

`cargo test`에는 이 저장소에 그림, 그림 팩, 키가 들어가지 않았는지 보는 검사가 들어 있습니다.

## 폴더

| 폴더 | 내용 |
|---|---|
| `crates/app` | 캐릭터 실행 파일 |
| `crates/pack` | 그림 팩 형식. 읽기와 쓰기, 암호화, 키 다루기 |
| `placeholder` | 대체 그림 팩 |

## 라이선스

- 코드: MIT (`LICENSE`)
- 그림: 코드 라이선스에 포함되지 않습니다. `LICENSE-ASSETS.md`를 봐 주세요.
