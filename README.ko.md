<p align="center">
  <img src="assets/banner.svg" alt="growTerm banner" width="100%"/>
</p>

[English](README.md)

자라나는 터미널 앱 — Rust로 만든 GPU 가속 터미널 에뮬레이터. macOS 지원.

## 설계 목표

- **Modular**: 각 모듈은 하나의 책임만 갖는다. 클립보드 복사를 고칠 때 VT 파서를 몰라도 된다.
- **Testable**: 순수 함수와 상태 머신은 단위 테스트로, 모듈 간 연동은 통합 테스트로 검증한다.
- **Evolvable**: 가역적 구조라서 안심하고 변화하고, 성장하고, 진화할 수 있다.

## 특징

- **GPU 렌더링** — wgpu 기반 2-pass 렌더링 (배경 + 글리프)
- **한글 지원** — IME 입력 + 프리에딧 오버레이, 와이드 문자 처리, D2Coding 폰트
- **탭** — Cmd+T/W로 열기/닫기, Cmd+1-9로 전환, Cmd+Shift+[/]로 순환, 탭바 클릭, 새 탭은 현재 작업 디렉토리 상속
- **VT 파싱** — SGR 속성 (볼드, 딤, 이탤릭, 밑줄, 취소선, 반전), 256/RGB 컬러, 커서 이동, 화면 지우기
- **TUI 앱 지원** — 대체 화면, 스크롤 영역, 마우스 트래킹 (SGR), 브래킷 붙여넣기, 동기 출력, 커서 가시성 (DECTCEM)
- **스크롤백** — 10,000줄 히스토리, Cmd+PageUp/PageDown, 드래그 가능한 자동 숨김 스크롤바
- **복사 모드** — Vim 스타일 복사 모드 (Cmd+Shift+C), hjkl 내비게이션
- **마우스 선택 & 클립보드** — 와이드 문자 인식 드래그 선택, Cmd+C/V, Cmd+A로 입력 줄 복사
- **URL 하이라이트** — Cmd+호버로 URL 밑줄 표시 및 감지
- **뽀모도로 타이머** — 설정 가능한 작업/휴식 사이클, 입력 차단 (기본 25분/3분)
- **응답 타이머** — 탭별 명령 응답 시간 측정
- **코칭** — AI 코칭 레이어, 자동 줄바꿈 (Claude CLI 사용)
- **폰트 줌** — Cmd+=/- 로 크기 조절 (8pt–72pt)
- **박스 드로잉** — 가는 선, 굵은 선, 이중 선, 둥근 모서리 문자의 기하학적 렌더링
- **키보드** — xterm 스타일 인코딩, Shift/Ctrl/Alt 조합키, kitty 키보드 프로토콜

## 단축키

| 단축키 | 동작 |
|---|---|
| Cmd+N | 새 창 |
| Cmd+T | 새 탭 |
| Cmd+W | 탭 닫기 |
| Cmd+1–9 | 탭 번호로 전환 |
| Cmd+Shift+[ / ] | 이전 / 다음 탭 |
| Cmd+C | 복사 |
| Cmd+V | 붙여넣기 |
| Cmd+A | 입력 줄 클립보드 복사 |
| Cmd+= / Cmd+- | 줌 인 / 아웃 |
| Cmd+PageUp/Down | 한 페이지 스크롤 |
| Cmd+Home / End | 최상단 / 최하단 스크롤 |
| Cmd+Click | 커서 아래 URL 열기 |
| `` ` `` 또는 Cmd+Shift+C | 복사 모드 진입 / 종료 |

### 복사 모드

| 키 | 동작 |
|---|---|
| j / k | 1줄 아래 / 위 이동 |
| h / l | 10줄 위 / 아래 이동 |
| v | 비주얼 모드 토글 (여러 줄 범위 선택) |
| Cmd+C | 선택 영역 복사 후 복사 모드 종료 |

## 설정

설정 파일은 `~/.config/growterm/config.toml`에 저장된다. 모든 항목은 선택적이며, 생략하면 기본값이 사용된다.

```toml
font_family = "FiraCodeNerdFontMono-Retina"  # 폰트 이름
font_size = 32.0                              # 폰트 크기 (pt)
pomodoro = false                              # 뽀모도로 타이머 활성화
pomodoro_work_minutes = 25                    # 작업 시간 (분)
pomodoro_break_minutes = 3                    # 휴식 시간 (분)
response_timer = false                        # 응답 타이머 활성화
coaching = true                               # AI 코칭 활성화
coaching_command = "claude -p ..."            # 커스텀 코칭 명령어
transparent_tab_bar = false                   # 탭/타이틀바 투명화
header_opacity = 0.8                          # 탭바 불투명도 (0.0–1.0)
window_width = 800                            # 초기 윈도우 너비
window_height = 600                           # 초기 윈도우 높이
window_x = 100                                # 윈도우 x 위치
window_y = 50                                 # 윈도우 y 위치

[copy_mode_keys]
down = "j"                                    # 단일 키 또는 배열
up = "k"
visual = "v"
half_page_down = ["h", "d"]
half_page_up = ["l", "u"]
yank = "y"
exit = ["q", "Escape", "`"]
```

기존 개별 설정 파일(`pomodoro_enabled` 등)은 첫 로드 시 `config.toml`로 자동 마이그레이션된다.

### 코칭

뽀모도로 휴식이 시작되면, growTerm이 작업 세션 동안의 터미널 출력을 캡처해서 AI에게 코칭 피드백을 요청한다. 응답은 휴식 중 오버레이로 표시된다.

**기본 동작** — Claude CLI(`claude -p`)를 사용하며, 기본 시스템 프롬프트:

> 당신은 코치입니다. 판단하거나 가르치지 마세요. 관찰한 내용을 짧게 알려주고, 사용자가 미처 보지 못했을 부분을 질문으로 던져주세요. 한국어로 3-4문장 이내로 답하세요.

**다른 모델 사용** — `coaching_command`에 원하는 셸 명령을 설정한다. 터미널 출력이 stdin으로 전달된다.

```toml
# 예: GPT-4o 사용
coaching_command = "openai api chat.completions.create -m gpt-4o"

# 예: 로컬 Ollama 모델 사용
coaching_command = "ollama run llama3"

# 예: Claude에 커스텀 프롬프트 사용
coaching_command = "claude --system-prompt 'You are a concise code reviewer.' -p"
```

## 아키텍처

```
키 입력 → 입력 인코딩 → PTY
                         ↓
                      VT 파서
                         ↓
                        그리드
                         ↓
                    렌더 커맨드
                         ↓
                     GPU 렌더링 → 화면
```

### 공유 타입
모든 모듈이 함께 쓰는 데이터 타입 (`Cell`, `Color`, `KeyEvent` 등). 모듈끼리 대화하기 위한 공통 언어.

### 입력 인코딩
키 입력을 셸이 알아듣는 바이트로 번역한다.

`Ctrl+C → \x03` · `↑ → \x1b[A`

### PTY
PTY(Pseudo-Terminal, 가상 터미널)는 우리 앱과 셸 사이의 파이프. 셸을 속여서 진짜 터미널에 연결된 것처럼 만든다.

`\x03 → shell → \x1b[31mHello`

### VT 파서
셸이 보낸 바이트를 구조화된 명령으로 해석한다.

`\x1b[31mHi → [SetColor(Red), Print('H'), Print('i')]`

### 그리드
스프레드시트 같은 2D 셀 격자. 각 문자를 위치와 스타일과 함께 저장한다. 스크롤백 히스토리도 보관.

`[SetColor(Red), Print('H')] → grid[row=0][col=0] = 'H' (red)`

### 렌더 커맨드
그리드를 읽어서 그리기 목록을 만든다. 커서, 선택 영역, IME 오버레이도 덧붙인다.

`grid[0][0]='H'(red) → DrawCell { row:0, col:0, char:'H', fg:#FF0000, bg:#000000 }`

### GPU 렌더링
그리기 목록을 받아서 GPU로 실제 픽셀을 화면에 찍는다. 각 문자를 비트맵으로 만들어 윈도우 위에 합성.

`DrawCell { char:'H', fg:#FF0000 } → 화면의 픽셀`

### macOS
윈도우를 만들고, OS에서 마우스/키보드 이벤트를 받고, IME(한글 입력)를 처리해서 앱으로 넘긴다.

### 앱
지휘자. 모든 모듈을 연결한다: 키 입력이 들어오고, 셸 출력이 돌아오고, 그리드가 갱신되고, 화면이 다시 그려진다.

## 빌드 & 실행

```bash
cargo build --release
cargo run -p growterm-app
```

### macOS 앱으로 설치

```bash
./install.sh
```

릴리스 바이너리를 빌드하고 `growTerm.app`을 `/Applications`에 설치한다.

## 테스트

```bash
cargo test
```

717개 이상 테스트 (단위 + 통합).

## 요구사항

- Rust (stable)
- macOS (wgpu Metal 백엔드)

## 라이선스

MIT
