# Конфиденциальность

Джарвис работает на вашем компьютере. У проекта нет аккаунтов, телеметрии и
аналитики. Единственный сервер проекта — **Jarvis Cloud** (Cloudflare Worker,
код в папке `cloud/` этого репозитория). Через него идут запросы к модели и
облачной речи, пока вы не указали свой ключ. Он ничего не сохраняет из
содержимого: в базе лежат только дневные счётчики (сколько запросов, озвучек и
секунд распознавания на случайный номер устройства и сколько новых устройств с
IP-адреса за день).

## Что остаётся на компьютере

* **Речь до обращения.** Имя «Джарвис» распознаётся локально (Silero VAD и
  GigaAM); пока вы не позвали Джарвиса, звук никуда не уходит. Законченная
  просьба в разговоре распознаётся ещё и Yandex SpeechKit через Jarvis Cloud
  (выключается в «Настройки → Голос → Распознавание Яндекса»).
* **Настройки, история, журнал действий, кэш озвучки и модели** — в
  `%LOCALAPPDATA%\dev.jarvis.desktop` (на macOS — в папке приложения в
  `~/Library`).
* **Ключи API** — в диспетчере учётных данных Windows (на macOS — в связке
  ключей). В базу, журналы и интерфейс они не попадают.
* **Снимки экрана и звук** на диск не сохраняются.
* **Наставник** смотрит заголовок активного окна и, на YouTube, Instagram и
  VK, адрес открытой вкладки — только чтобы считать минуты коротких видео.
  Это остаётся на компьютере: в базе лежат минуты по сайтам, а не история
  просмотров.
* **Календарь** хранится в той же базе; без подключённого Google он не
  покидает компьютер.

## Что уходит в сеть и куда

Только то, что вы включили сами, и только туда, куда вы указали.

| Когда | Куда | Что |
|---|---|---|
| Вы просите что-то у Джарвиса без своего ключа (по умолчанию) | Jarvis Cloud → DeepSeek | То же, что ниже для провайдера; сервер пересылает запрос и считает его в дневном лимите |
| Вы говорите просьбу голосом, «Распознавание Яндекса» включено | Jarvis Cloud → Yandex SpeechKit (Казахстан) | Звук законченной просьбы, до 30 секунд |
| Вы просите что-то у Джарвиса, и в настройках есть ключ модели | Провайдер, которого вы выбрали: DeepSeek, Qwen (Alibaba Cloud Model Studio) или свой OpenAI-совместимый адрес | Текст просьбы и разговора, местное время, город (если он уже известен), заголовки активного и недавних окон. Снимок экрана — когда для вашей просьбы нужно посмотреть на экран |
| Выбран голосовой режим Realtime | Alibaba Cloud DashScope | Звук с микрофона во время разговора |
| Выбран облачный голос | Microsoft Azure Speech, ElevenLabs или Yandex SpeechKit (без своего ключа — через Jarvis Cloud) | Текст ответа для озвучки |
| Вы спрашиваете погоду или «что рядом» | ipwho.is, Open-Meteo | IP-адрес (по нему определяется город), координаты города |
| Вы просите что-то про GitHub | api.github.com | Запрос к вашему профилю и репозиториям |
| Вы включаете голос впервые | GitHub Releases проекта sherpa-onnx | Скачиваются голосовые модели |
| Вы подключили Google Календарь (свой OAuth-ключ из Google Cloud) | googleapis.com | События основного календаря: название, время, место, описание, цвет — в обе стороны, раз в несколько минут |
| Вы добавили своего Telegram-бота | api.telegram.org | Сообщения вашему чату: план дня, итог, напоминания, ответы Джарвиса; ваши сообщения боту |
| Карточки для ленты знаний (если есть ключ модели) | Ваш провайдер модели | Тема (программирование или английский) и список уже известных карточек — без ваших данных |
| Проверка обновлений (раз в шесть часов; выключается в «Настройки → Основное → Обновления») | github.com | Запрос файла `latest.json` этого репозитория |

Без своего ключа запросы к ИИ идут через Jarvis Cloud. Ключ уходит только на
тот адрес, для которого он сохранён. Сайты, которые вы просите открыть,
открываются в вашем браузере.

На то, что вы отправляете выбранным сервисам, распространяются их собственные
правила конфиденциальности: Cloudflare (Jarvis Cloud), DeepSeek, Alibaba Cloud, Microsoft, ElevenLabs,
Yandex Cloud, ipwho.is, Open-Meteo, GitHub.

## Как удалить всё

Удалите Джарвиса («Параметры → Приложения» или ярлык удаления) и отметьте
«Настройки, история и ключи API». Тогда исчезнут и папка с данными, и ключи из
диспетчера учётных данных.

---

## Privacy (English)

Jarvis runs on your computer. The project has no accounts, telemetry or
analytics. Its only server is Jarvis Cloud (a Cloudflare Worker, source in
`cloud/`), which forwards model and speech requests while the user has no key
of their own and stores nothing but daily usage counters per random device id
(and new devices per IP address per day).

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or
operating it. The wake word is recognised locally; nothing is sent until the
user addresses Jarvis. Without an own key, requests go through Jarvis Cloud to
DeepSeek and Yandex SpeechKit (the audio of a finished spoken request, up to
30 s, when «Распознавание Яндекса» is on). Otherwise requests go only to the
services the user configures: the AI provider for which the user saved an API
key (the request text, conversation, local time, city if
known, titles of the active and recent windows, and a screenshot when the task
needs one), optional cloud voices (reply text), the optional realtime voice
mode (microphone audio), and, when asked, weather (ipwho.is, Open-Meteo) and
GitHub; when the user connects them, Google Calendar (events of the primary
calendar, both ways, with the user's own OAuth client) and the user's own
Telegram bot (messages to and from the bound chat). The mentor reads window
titles and, on YouTube, Instagram and VK, the active tab's address locally to
count short-video minutes; it stores minutes per site, not a watch history.
The app checks this repository on GitHub for updates every six hours;
this can be turned off in the settings. Uninstalling with «Настройки, история
и ключи API» checked removes all local data and stored keys.
