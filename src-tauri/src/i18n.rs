//! Text the backend shows by itself — desktop notifications, the tray menu, release-check
//! and reminder lines, file-picker titles, the play-failure headline and native dialogs — in
//! the interface language (`app_language` in settings.json, else
//! the system's). Log lines stay in English. The frontend has its own catalogs (src/i18n/).

use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Interface languages, in the column order of the tables below.
pub const LANGUAGES: [&str; 12] = ["en", "fr", "ja", "ko", "zh", "pt", "ru", "hi", "es", "de", "ar", "it"];

const REFRESH_EVERY: Duration = Duration::from_secs(3);

/// Key, then the text in each of `LANGUAGES`. `{name}` marks a value filled in by `fill`.
const TEXTS: &[(&str, [&str; 12])] = &[
    ("episode_out", ["Episode {episode} is out", "L’épisode {episode} est disponible", "第{episode}話が公開されました", "{episode}화가 공개됐어요", "第 {episode} 集已更新", "O episódio {episode} saiu", "Серия {episode} уже вышла", "एपिसोड {episode} आ गया है", "Ya salió el episodio {episode}", "Folge {episode} ist da", "صدرت الحلقة {episode}", "È uscito l’episodio {episode}"]),
    ("two_out", ["Episodes {first} and {second} are out", "Les épisodes {first} et {second} sont disponibles", "第{first}話と第{second}話が公開されました", "{first}화, {second}화가 공개됐어요", "第 {first} 集和第 {second} 集已更新", "Os episódios {first} e {second} saíram", "Вышли серии {first} и {second}", "एपिसोड {first} और {second} आ गए हैं", "Ya salieron los episodios {first} y {second}", "Folgen {first} und {second} sind da", "صدرت الحلقتان {first} و{second}", "Sono usciti gli episodi {first} e {second}"]),
    ("range_out", ["Episodes {first}–{last} are out", "Les épisodes {first} à {last} sont disponibles", "第{first}〜{last}話が公開されました", "{first}~{last}화가 공개됐어요", "第 {first}–{last} 集已更新", "Os episódios {first} a {last} saíram", "Вышли серии {first}–{last}", "एपिसोड {first}–{last} आ गए हैं", "Ya salieron los episodios {first} a {last}", "Folgen {first}–{last} sind da", "صدرت الحلقات {first}–{last}", "Sono usciti gli episodi {first}–{last}"]),
    ("open", ["Open", "Ouvrir", "開く", "열기", "打开", "Abrir", "Открыть", "खोलें", "Abrir", "Öffnen", "فتح", "Apri"]),
    ("found", ["Episode {episode} is out — {track}", "L’épisode {episode} est disponible — {track}", "第{episode}話が公開されました — {track}", "{episode}화가 공개됐어요 — {track}", "第 {episode} 集已更新 — {track}", "O episódio {episode} saiu — {track}", "Серия {episode} уже вышла — {track}", "एपिसोड {episode} आ गया है — {track}", "Ya salió el episodio {episode} — {track}", "Folge {episode} ist da — {track}", "صدرت الحلقة {episode} — {track}", "È uscito l’episodio {episode} — {track}"]),
    ("missing", ["Episode {episode} has aired, but no source has it yet — {wanted}", "L’épisode {episode} a été diffusé, mais aucune source ne l’a encore — {wanted}", "第{episode}話は放送されましたが、まだどのソースにもありません — {wanted}", "{episode}화가 방영됐지만 아직 제공하는 소스가 없어요 — {wanted}", "第 {episode} 集已播出，但还没有片源 — {wanted}", "O episódio {episode} já foi ao ar, mas nenhuma fonte o tem ainda — {wanted}", "Серия {episode} уже в эфире, но ни в одном источнике её пока нет — {wanted}", "एपिसोड {episode} प्रसारित हो गया है, पर अभी किसी स्रोत पर नहीं है — {wanted}", "El episodio {episode} ya se emitió, pero ninguna fuente lo tiene aún — {wanted}", "Folge {episode} wurde ausgestrahlt, aber noch keine Quelle hat sie — {wanted}", "عُرضت الحلقة {episode}، لكن لا يوفّرها أي مصدر بعد — {wanted}", "L’episodio {episode} è andato in onda, ma nessuna fonte lo ha ancora — {wanted}"]),
    ("track_sub_in", ["Japanese audio · {language} subtitles", "Audio japonais · sous-titres en {language}", "日本語音声 · {language}字幕", "일본어 음성 · {language} 자막", "日语音轨 · {language}字幕", "Áudio japonês · legendas em {language}", "Японский звук · субтитры: {language}", "जापानी ऑडियो · {language} सबटाइटल", "Audio japonés · subtítulos en {language}", "Japanischer Ton · Untertitel: {language}", "صوت ياباني · ترجمة باللغة {language}", "Audio giapponese · sottotitoli in {language}"]),
    ("track_sub", ["Japanese audio · subtitles", "Audio japonais · sous-titres", "日本語音声 · 字幕", "일본어 음성 · 자막", "日语音轨 · 字幕", "Áudio japonês · legendas", "Японский звук · субтитры", "जापानी ऑडियो · सबटाइटल", "Audio japonés · subtítulos", "Japanischer Ton · Untertitel", "صوت ياباني · ترجمة", "Audio giapponese · sottotitoli"]),
    ("track_dub_in", ["{language} audio", "Audio en {language}", "{language}音声", "{language} 음성", "{language}配音", "Áudio em {language}", "Звук: {language}", "{language} ऑडियो", "Audio en {language}", "Ton: {language}", "صوت باللغة {language}", "Audio in {language}"]),
    ("track_dub", ["dubbed audio", "audio doublé", "吹替音声", "더빙 음성", "配音", "áudio dublado", "дубляж", "डब ऑडियो", "audio doblado", "Synchronfassung", "صوت مدبلج", "audio doppiato"]),
    ("wanted_sub", ["{languages} subtitles", "sous-titres en {languages}", "{languages}字幕", "{languages} 자막", "{languages}字幕", "legendas em {languages}", "субтитры: {languages}", "{languages} सबटाइटल", "subtítulos en {languages}", "Untertitel: {languages}", "ترجمة باللغة {languages}", "sottotitoli in {languages}"]),
    ("wanted_dub", ["{languages} audio", "audio en {languages}", "{languages}音声", "{languages} 음성", "{languages}配音", "áudio em {languages}", "звук: {languages}", "{languages} ऑडियो", "audio en {languages}", "Ton: {languages}", "صوت باللغة {languages}", "audio in {languages}"]),
    ("your_languages", ["your languages", "vos langues", "選んだ言語", "선택한 언어", "你的语言", "seus idiomas", "ваши языки", "आपकी भाषाएँ", "tus idiomas", "deine Sprachen", "لغاتك", "le tue lingue"]),
    ("or", ["{list} or {last}", "{list} ou {last}", "{list}または{last}", "{list} 또는 {last}", "{list}或{last}", "{list} ou {last}", "{list} или {last}", "{list} या {last}", "{list} o {last}", "{list} oder {last}", "{list} أو {last}", "{list} o {last}"]),
    ("mpv_back_title", ["Anix switched back to the web player", "Anix est revenu au lecteur web", "Anix はウェブプレーヤーに戻りました", "Anix가 웹 플레이어로 돌아갔어요", "Anix 已切回网页播放器", "O Anix voltou ao player web", "Anix вернулся к веб-плееру", "Anix वेब प्लेयर पर लौट आया", "Anix volvió al reproductor web", "Anix nutzt wieder den Web-Player", "عاد Anix إلى مشغّل الويب", "Anix è tornato al lettore web"]),
    ("mpv_back_body", ["Nobody answered “Is the video player showing correctly?” in time, so Anix uses the web player now. To try embedded mpv again: Settings › Playback › Player engine.", "Personne n’a répondu à « Le lecteur vidéo s’affiche-t-il correctement ? » à temps, Anix utilise donc le lecteur web. Pour réessayer mpv intégré : Paramètres › Lecture › Moteur du lecteur.", "「動画プレーヤーは正しく表示されていますか？」に時間内の回答がなかったため、ウェブプレーヤーを使っています。内蔵 mpv を再び試すには：設定 › 再生 › プレーヤーエンジン。", "“동영상 플레이어가 제대로 보이나요?”에 제때 답이 없어 지금은 웹 플레이어를 사용해요. 내장 mpv를 다시 쓰려면: 설정 › 재생 › 플레이어 엔진.", "“视频播放器显示正常吗？”未及时回答，因此 Anix 现在使用网页播放器。如需再次尝试内置 mpv：设置 › 播放 › 播放器引擎。", "Ninguém respondeu a “O player de vídeo aparece corretamente?” a tempo, então o Anix usa o player web agora. Para tentar o mpv integrado de novo: Configurações › Reprodução › Mecanismo do player.", "На вопрос «Видеоплеер отображается правильно?» не ответили вовремя, поэтому Anix использует веб-плеер. Чтобы снова попробовать встроенный mpv: Настройки › Воспроизведение › Движок плеера.", "“क्या वीडियो प्लेयर सही दिख रहा है?” का जवाब समय पर नहीं मिला, इसलिए Anix अब वेब प्लेयर इस्तेमाल करता है। एम्बेडेड mpv फिर से आज़माने के लिए: सेटिंग्स › प्लेबैक › प्लेयर इंजन।", "Nadie respondió a «¿El reproductor de vídeo se ve bien?» a tiempo, así que Anix usa ahora el reproductor web. Para volver a probar mpv integrado: Ajustes › Reproducción › Motor del reproductor.", "Die Frage „Wird der Videoplayer richtig angezeigt?“ blieb unbeantwortet, daher nutzt Anix jetzt den Web-Player. Eingebettetes mpv wieder ausprobieren: Einstellungen › Wiedergabe › Player-Engine.", "لم تتم الإجابة عن «هل يظهر مشغّل الفيديو بشكل صحيح؟» في الوقت المناسب، لذا يستخدم Anix الآن مشغّل الويب. لتجربة mpv المدمج مجددًا: الإعدادات › التشغيل › محرّك المشغّل.", "Nessuno ha risposto in tempo a «Il lettore video si vede correttamente?», quindi ora Anix usa il lettore web. Per riprovare mpv integrato: Impostazioni › Riproduzione › Motore del lettore."]),
    ("mpv_auto_title", ["Anix now uses embedded mpv", "Anix utilise maintenant mpv intégré", "Anix は内蔵 mpv を使うようになりました", "Anix가 이제 내장 mpv를 사용해요", "Anix 现在使用内置 mpv", "O Anix agora usa o mpv integrado", "Anix теперь использует встроенный mpv", "Anix अब एम्बेडेड mpv इस्तेमाल करता है", "Anix usa ahora mpv integrado", "Anix nutzt jetzt eingebettetes mpv", "يستخدم Anix الآن mpv المدمج", "Anix ora usa mpv integrato"]),
    ("mpv_auto_body", ["The web player crashed during playback, so Anix switched to embedded mpv. To change it: Settings › Playback › Player engine.", "Le lecteur web a planté pendant la lecture, Anix est donc passé à mpv intégré. Pour changer : Paramètres › Lecture › Moteur du lecteur.", "再生中にウェブプレーヤーがクラッシュしたため、内蔵 mpv に切り替えました。変更するには：設定 › 再生 › プレーヤーエンジン。", "재생 중 웹 플레이어가 멈춰서 내장 mpv로 바꿨어요. 바꾸려면: 설정 › 재생 › 플레이어 엔진.", "网页播放器在播放时崩溃，因此 Anix 已切换到内置 mpv。如需更改：设置 › 播放 › 播放器引擎。", "O player web travou durante a reprodução, então o Anix mudou para o mpv integrado. Para mudar: Configurações › Reprodução › Mecanismo do player.", "Веб-плеер аварийно завершился во время воспроизведения, поэтому Anix переключился на встроенный mpv. Изменить: Настройки › Воспроизведение › Движок плеера.", "प्लेबैक के दौरान वेब प्लेयर क्रैश हो गया, इसलिए Anix एम्बेडेड mpv पर चला गया। बदलने के लिए: सेटिंग्स › प्लेबैक › प्लेयर इंजन।", "El reproductor web falló durante la reproducción, así que Anix cambió a mpv integrado. Para cambiarlo: Ajustes › Reproducción › Motor del reproductor.", "Der Web-Player ist während der Wiedergabe abgestürzt, daher nutzt Anix jetzt eingebettetes mpv. Ändern: Einstellungen › Wiedergabe › Player-Engine.", "تعطّل مشغّل الويب أثناء التشغيل، لذا انتقل Anix إلى mpv المدمج. للتغيير: الإعدادات › التشغيل › محرّك المشغّل.", "Il lettore web si è bloccato durante la riproduzione, quindi Anix è passato a mpv integrato. Per cambiarlo: Impostazioni › Riproduzione › Motore del lettore."]),
    ("page_reloaded_title", ["Anix reloaded its window", "Anix a rechargé sa fenêtre", "Anix がウィンドウを再読み込みしました", "Anix가 창을 다시 불러왔어요", "Anix 已重新加载窗口", "O Anix recarregou a janela", "Anix перезагрузил окно", "Anix ने अपनी विंडो फिर से लोड की", "Anix recargó su ventana", "Anix hat sein Fenster neu geladen", "أعاد Anix تحميل نافذته", "Anix ha ricaricato la finestra"]),
    ("page_reloaded_body", ["The web engine that draws the window stopped unexpectedly, so Anix stopped playback and reloaded the page.", "Le moteur web qui affiche la fenêtre s’est arrêté de façon inattendue, Anix a donc arrêté la lecture et rechargé la page.", "ウィンドウを描画するウェブエンジンが突然停止したため、再生を止めてページを再読み込みしました。", "창을 그리는 웹 엔진이 갑자기 멈춰서 재생을 멈추고 페이지를 다시 불러왔어요.", "绘制窗口的网页引擎意外停止，因此 Anix 已停止播放并重新加载页面。", "O mecanismo web que desenha a janela parou de forma inesperada, então o Anix parou a reprodução e recarregou a página.", "Веб-движок, который рисует окно, неожиданно остановился, поэтому Anix остановил воспроизведение и перезагрузил страницу.", "विंडो दिखाने वाला वेब इंजन अचानक रुक गया, इसलिए Anix ने प्लेबैक रोककर पेज फिर से लोड किया।", "El motor web que dibuja la ventana se detuvo de forma inesperada, así que Anix detuvo la reproducción y recargó la página.", "Die Web-Engine, die das Fenster zeichnet, wurde unerwartet beendet, daher hat Anix die Wiedergabe gestoppt und die Seite neu geladen.", "توقف محرّك الويب الذي يرسم النافذة فجأة، لذا أوقف Anix التشغيل وأعاد تحميل الصفحة.", "Il motore web che disegna la finestra si è fermato all’improvviso, quindi Anix ha interrotto la riproduzione e ricaricato la pagina."]),
    ("page_crash_loop", ["Anix’s window stopped working several times in a row, so it is not reloaded again. Anix will close now. Start it again; if this keeps happening, change Settings › Playback › Player engine.", "La fenêtre d’Anix a cessé de fonctionner plusieurs fois de suite, elle ne sera plus rechargée. Anix va se fermer. Relancez-le ; si cela continue, changez Paramètres › Lecture › Moteur du lecteur.", "Anix のウィンドウが続けて何度も停止したため、これ以上再読み込みしません。Anix を終了します。もう一度起動してください。繰り返す場合は 設定 › 再生 › プレーヤーエンジン を変更してください。", "Anix 창이 연달아 여러 번 멈춰서 더 이상 다시 불러오지 않아요. Anix를 종료합니다. 다시 실행해 주세요. 계속되면 설정 › 재생 › 플레이어 엔진을 바꿔 보세요.", "Anix 窗口连续多次停止工作，因此不再重新加载。Anix 将关闭。请重新启动；如果仍然发生，请更改 设置 › 播放 › 播放器引擎。", "A janela do Anix parou de funcionar várias vezes seguidas, então não será recarregada de novo. O Anix vai fechar. Abra-o de novo; se continuar, mude Configurações › Reprodução › Mecanismo do player.", "Окно Anix несколько раз подряд перестало работать, поэтому больше не перезагружается. Anix сейчас закроется. Запустите его снова; если это повторится, измените Настройки › Воспроизведение › Движок плеера.", "Anix की विंडो लगातार कई बार रुक गई, इसलिए उसे फिर से लोड नहीं किया जाएगा। Anix अब बंद होगा। इसे फिर से शुरू करें; अगर ऐसा होता रहे तो सेटिंग्स › प्लेबैक › प्लेयर इंजन बदलें।", "La ventana de Anix dejó de funcionar varias veces seguidas, así que no se recargará más. Anix se cerrará ahora. Vuelve a abrirlo; si sigue pasando, cambia Ajustes › Reproducción › Motor del reproductor.", "Das Fenster von Anix ist mehrmals hintereinander ausgefallen und wird nicht mehr neu geladen. Anix wird jetzt beendet. Starte es neu; passiert das weiter, ändere Einstellungen › Wiedergabe › Player-Engine.", "توقفت نافذة Anix عن العمل عدة مرات متتالية، لذا لن يُعاد تحميلها مرة أخرى. سيُغلق Anix الآن. شغّله من جديد؛ وإذا تكرر ذلك فغيّر الإعدادات › التشغيل › محرّك المشغّل.", "La finestra di Anix ha smesso di funzionare più volte di seguito, quindi non verrà ricaricata. Anix ora si chiuderà. Riavvialo; se continua, cambia Impostazioni › Riproduzione › Motore del lettore."]),
    ("list_separator", [", ", ", ", "、", ", ", "、", ", ", ", ", ", ", ", ", ", ", "، ", ", "]),
    ("reminder_one", ["Episode {episode} airs in {minutes} minute", "L’épisode {episode} est diffusé dans {minutes} minute", "第{episode}話はあと{minutes}分で放送", "{episode}화가 {minutes}분 후 방영돼요", "第 {episode} 集将在 {minutes} 分钟后播出", "O episódio {episode} vai ao ar em {minutes} minuto", "Серия {episode} выйдет в эфир через {minutes} мин", "एपिसोड {episode} {minutes} मिनट में प्रसारित होगा", "El episodio {episode} se emite en {minutes} minuto", "Folge {episode} läuft in {minutes} Minute", "تُعرض الحلقة {episode} بعد {minutes} د", "L’episodio {episode} va in onda tra {minutes} minuto"]),
    ("reminder_other", ["Episode {episode} airs in {minutes} minutes", "L’épisode {episode} est diffusé dans {minutes} minutes", "第{episode}話はあと{minutes}分で放送", "{episode}화가 {minutes}분 후 방영돼요", "第 {episode} 集将在 {minutes} 分钟后播出", "O episódio {episode} vai ao ar em {minutes} minutos", "Серия {episode} выйдет в эфир через {minutes} мин", "एपिसोड {episode} {minutes} मिनट में प्रसारित होगा", "El episodio {episode} se emite en {minutes} minutos", "Folge {episode} läuft in {minutes} Minuten", "تُعرض الحلقة {episode} بعد {minutes} د", "L’episodio {episode} va in onda tra {minutes} minuti"]),
    ("tray_open", ["Open Anix", "Ouvrir Anix", "Anix を開く", "Anix 열기", "打开 Anix", "Abrir o Anix", "Открыть Anix", "Anix खोलें", "Abrir Anix", "Anix öffnen", "فتح Anix", "Apri Anix"]),
    ("tray_airing_next", ["Airing next", "Prochaines diffusions", "次の放送", "다음 방영", "即将播出", "Próximos episódios", "Скоро в эфире", "आगे प्रसारण", "Próximas emisiones", "Als Nächstes", "يُعرض قريبًا", "Prossime uscite"]),
    ("tray_quit", ["Quit Anix", "Quitter Anix", "Anix を終了", "Anix 종료", "退出 Anix", "Sair do Anix", "Выйти из Anix", "Anix बंद करें", "Salir de Anix", "Anix beenden", "إنهاء Anix", "Esci da Anix"]),
    ("tray_episode", ["Ep {episode}", "Ép. {episode}", "第{episode}話", "{episode}화", "第 {episode} 集", "Ep. {episode}", "Серия {episode}", "एपि. {episode}", "Ep. {episode}", "F. {episode}", "ح {episode}", "Ep. {episode}"]),
    ("today", ["today {time}", "aujourd’hui {time}", "今日 {time}", "오늘 {time}", "今天 {time}", "hoje {time}", "сегодня {time}", "आज {time}", "hoy {time}", "heute {time}", "اليوم {time}", "oggi {time}"]),
    ("tomorrow", ["tomorrow {time}", "demain {time}", "明日 {time}", "내일 {time}", "明天 {time}", "amanhã {time}", "завтра {time}", "कल {time}", "mañana {time}", "morgen {time}", "غدًا {time}", "domani {time}"]),
    ("day0", ["Sun", "dim.", "日曜", "일요일", "周日", "dom.", "вс", "रवि", "dom", "So", "الأحد", "dom"]),
    ("day1", ["Mon", "lun.", "月曜", "월요일", "周一", "seg.", "пн", "सोम", "lun", "Mo", "الاثنين", "lun"]),
    ("day2", ["Tue", "mar.", "火曜", "화요일", "周二", "ter.", "вт", "मंगल", "mar", "Di", "الثلاثاء", "mar"]),
    ("day3", ["Wed", "mer.", "水曜", "수요일", "周三", "qua.", "ср", "बुध", "mié", "Mi", "الأربعاء", "mer"]),
    ("day4", ["Thu", "jeu.", "木曜", "목요일", "周四", "qui.", "чт", "गुरु", "jue", "Do", "الخميس", "gio"]),
    ("day5", ["Fri", "ven.", "金曜", "금요일", "周五", "sex.", "пт", "शुक्र", "vie", "Fr", "الجمعة", "ven"]),
    ("day6", ["Sat", "sam.", "土曜", "토요일", "周六", "sáb.", "сб", "शनि", "sáb", "Sa", "السبت", "sab"]),
    ("play_verify", ["The site asks you to pass its check first. Press Verify, then the episode is tried again.", "Le site vous demande d’abord de passer sa vérification. Appuyez sur Vérifier, puis l’épisode sera réessayé.", "サイトが先に確認を求めています。「確認」を押すと、もう一度このエピソードを試します。", "사이트에서 먼저 확인을 요구해요. 확인을 누르면 이 에피소드를 다시 시도해요.", "该网站要求你先通过它的验证。点击“验证”后会重新尝试这一集。", "O site pede que você passe pela verificação dele primeiro. Clique em Verificar e o episódio será tentado de novo.", "Сайт просит сначала пройти его проверку. Нажмите «Подтвердить», и серия будет загружена снова.", "साइट पहले अपनी जाँच पूरी करने को कह रही है। पुष्टि करें दबाएँ, फिर एपिसोड दोबारा आज़माया जाएगा।", "El sitio te pide que pases primero su verificación. Pulsa Verificar y se volverá a intentar el episodio.", "Die Website verlangt zuerst ihre Prüfung. Klicke auf Bestätigen, dann wird die Folge erneut versucht.", "يطلب الموقع اجتياز التحقق أولًا. اضغط تحقّق، ثم ستُجرَّب الحلقة مجددًا.", "Il sito chiede prima di superare il suo controllo. Premi Verifica e l’episodio verrà riprovato."]),
    ("play_removed", ["The site removed this video.", "Le site a supprimé cette vidéo.", "サイトがこの動画を削除しました。", "사이트에서 이 동영상을 삭제했어요.", "网站已删除此视频。", "O site removeu este vídeo.", "Сайт удалил это видео.", "साइट ने यह वीडियो हटा दिया है।", "El sitio eliminó este vídeo.", "Die Website hat dieses Video entfernt.", "أزال الموقع هذا الفيديو.", "Il sito ha rimosso questo video."]),
    ("play_unreadable_host", ["This episode is only on video hosts the extensions can't read ({hosts}).", "Cet épisode n’est que sur des hébergeurs vidéo que les extensions ne savent pas lire ({hosts}).", "このエピソードは、拡張機能が読み取れない動画ホストにしかありません（{hosts}）。", "이 에피소드는 확장 프로그램이 읽을 수 없는 동영상 호스트에만 있어요 ({hosts}).", "这一集只在扩展无法读取的视频托管站上（{hosts}）。", "Este episódio só está em hospedagens de vídeo que as extensões não conseguem ler ({hosts}).", "Эта серия есть только на видеохостингах, которые расширения не умеют читать ({hosts}).", "यह एपिसोड सिर्फ़ उन वीडियो होस्ट पर है जिन्हें एक्सटेंशन पढ़ नहीं सकते ({hosts})।", "Este episodio solo está en servidores de vídeo que las extensiones no pueden leer ({hosts}).", "Diese Folge gibt es nur bei Video-Hostern, die die Erweiterungen nicht lesen können ({hosts}).", "هذه الحلقة موجودة فقط على مستضيفي فيديو لا تستطيع الإضافات قراءتهم ({hosts}).", "Questo episodio è solo su host video che le estensioni non sanno leggere ({hosts})."]),
    ("play_too_slow", ["The sites are too slow to answer right now. Try again in a moment.", "Les sites sont trop lents à répondre en ce moment. Réessayez dans un instant.", "サイトの応答が今は遅すぎます。少ししてからもう一度試してください。", "사이트 응답이 지금 너무 느려요. 잠시 후 다시 시도하세요.", "网站目前响应太慢。请稍后再试。", "Os sites estão lentos demais para responder agora. Tente de novo daqui a pouco.", "Сайты сейчас отвечают слишком медленно. Попробуйте чуть позже.", "साइटें अभी जवाब देने में बहुत धीमी हैं। थोड़ी देर में फिर आज़माएँ।", "Los sitios tardan demasiado en responder ahora mismo. Vuelve a intentarlo en un momento.", "Die Websites antworten gerade zu langsam. Versuch es gleich nochmal.", "المواقع بطيئة جدًا في الرد الآن. حاول مجددًا بعد قليل.", "I siti sono troppo lenti a rispondere in questo momento. Riprova tra un attimo."]),
    ("play_unreachable", ["The sites could not be reached. Your network may block them; DNS over HTTPS in Settings › Network can help.", "Les sites sont injoignables. Votre réseau les bloque peut-être ; DNS over HTTPS dans Paramètres › Réseau peut aider.", "サイトに接続できませんでした。ネットワークがブロックしている可能性があります。設定 › ネットワークの DNS over HTTPS が役立つことがあります。", "사이트에 연결할 수 없었어요. 네트워크에서 막고 있을 수 있어요. 설정 › 네트워크의 DNS over HTTPS가 도움이 될 수 있어요.", "无法连接到这些网站。你的网络可能屏蔽了它们；设置 › 网络 中的 DNS over HTTPS 可能有帮助。", "Não foi possível acessar os sites. Sua rede pode estar bloqueando-os; o DNS over HTTPS em Configurações › Rede pode ajudar.", "Не удалось связаться с сайтами. Возможно, ваша сеть их блокирует; помочь может DNS over HTTPS в Настройки › Сеть.", "साइटों तक पहुँचा नहीं जा सका। हो सकता है आपका नेटवर्क उन्हें रोक रहा हो; सेटिंग्स › नेटवर्क में DNS over HTTPS मदद कर सकता है।", "No se pudo acceder a los sitios. Puede que tu red los bloquee; DNS over HTTPS en Ajustes › Red puede ayudar.", "Die Websites sind nicht erreichbar. Vielleicht blockiert dein Netzwerk sie; DNS over HTTPS unter Einstellungen › Netzwerk kann helfen.", "تعذّر الوصول إلى المواقع. قد تحجبها شبكتك؛ ويمكن أن يساعد DNS over HTTPS في الإعدادات › الشبكة.", "Impossibile raggiungere i siti. La tua rete potrebbe bloccarli; DNS over HTTPS in Impostazioni › Rete può aiutare."]),
    ("play_not_listed", ["No installed extension has this episode.", "Aucune extension installée n’a cet épisode.", "インストール済みの拡張機能にこのエピソードはありません。", "설치된 확장 프로그램 중 이 에피소드가 있는 것이 없어요.", "已安装的扩展都没有这一集。", "Nenhuma extensão instalada tem este episódio.", "Ни в одном установленном расширении нет этой серии.", "किसी भी इंस्टॉल किए गए एक्सटेंशन में यह एपिसोड नहीं है।", "Ninguna extensión instalada tiene este episodio.", "Keine installierte Erweiterung hat diese Folge.", "لا توجد هذه الحلقة في أي إضافة مثبّتة.", "Nessuna estensione installata ha questo episodio."]),
    ("play_not_listed_dub", ["No installed extension has this episode in dub.", "Aucune extension installée n’a cet épisode en version doublée.", "インストール済みの拡張機能に、このエピソードの吹替版はありません。", "설치된 확장 프로그램 중 이 에피소드의 더빙이 있는 것이 없어요.", "已安装的扩展都没有这一集的配音版。", "Nenhuma extensão instalada tem este episódio dublado.", "Ни в одном установленном расширении нет этой серии с дубляжом.", "किसी भी इंस्टॉल किए गए एक्सटेंशन में यह एपिसोड डब में नहीं है।", "Ninguna extensión instalada tiene este episodio doblado.", "Keine installierte Erweiterung hat diese Folge synchronisiert.", "لا توجد هذه الحلقة مدبلجةً في أي إضافة مثبّتة.", "Nessuna estensione installata ha questo episodio doppiato."]),
    ("play_other", ["The extensions answered, but none gave a stream that plays.", "Les extensions ont répondu, mais aucune n’a fourni de flux lisible.", "拡張機能は応答しましたが、再生できるストリームはありませんでした。", "확장 프로그램이 응답했지만 재생되는 스트림을 준 것이 없어요.", "扩展有响应，但没有一个提供可播放的视频流。", "As extensões responderam, mas nenhuma deu um stream que reproduza.", "Расширения ответили, но ни одно не дало воспроизводимый поток.", "एक्सटेंशन ने जवाब दिया, पर किसी ने भी चलने लायक स्ट्रीम नहीं दी।", "Las extensiones respondieron, pero ninguna dio un stream que se reproduzca.", "Die Erweiterungen haben geantwortet, aber keine lieferte einen abspielbaren Stream.", "ردّت الإضافات، لكن لم تقدّم أيٌّ منها بثًّا يعمل.", "Le estensioni hanno risposto, ma nessuna ha fornito uno stream riproducibile."]),
    ("play_unknown_provider", ["The server “{name}” is no longer available. Choose another server.", "Le serveur « {name} » n’est plus disponible. Choisissez un autre serveur.", "サーバー「{name}」は利用できなくなりました。別のサーバーを選んでください。", "서버 ‘{name}’은(는) 더 이상 사용할 수 없어요. 다른 서버를 고르세요.", "服务器“{name}”已不可用。请选择其他服务器。", "O servidor “{name}” não está mais disponível. Escolha outro servidor.", "Сервер «{name}» больше недоступен. Выберите другой сервер.", "सर्वर “{name}” अब उपलब्ध नहीं है। कोई दूसरा सर्वर चुनें।", "El servidor «{name}» ya no está disponible. Elige otro servidor.", "Der Server „{name}“ ist nicht mehr verfügbar. Wähle einen anderen Server.", "الخادم «{name}» لم يعد متاحًا. اختر خادمًا آخر.", "Il server «{name}» non è più disponibile. Scegli un altro server."]),
    ("play_no_language", ["No enabled plugin offers {wanted}. Choose other languages, or allow plugins in other languages, in Settings › Sources.", "Aucun plugin activé ne propose {wanted}. Choisissez d’autres langues, ou autorisez les plugins dans d’autres langues, dans Paramètres › Sources.", "有効なプラグインに{wanted}を提供するものはありません。設定 › ソースで別の言語を選ぶか、ほかの言語のプラグインを許可してください。", "{wanted}을(를) 제공하는 사용 중인 플러그인이 없어요. 설정 › 소스에서 다른 언어를 고르거나 다른 언어의 플러그인을 허용하세요.", "没有已启用的插件提供{wanted}。请在 设置 › 片源 中选择其他语言，或允许其他语言的插件。", "Nenhum plugin ativado oferece {wanted}. Escolha outros idiomas, ou permita plugins em outros idiomas, em Configurações › Fontes.", "Ни один включённый плагин не предлагает {wanted}. Выберите другие языки или разрешите плагины на других языках в Настройки › Источники.", "कोई भी चालू प्लगइन {wanted} नहीं देता। सेटिंग्स › स्रोत में दूसरी भाषाएँ चुनें, या दूसरी भाषाओं के प्लगइन की अनुमति दें।", "Ningún plugin activado ofrece {wanted}. Elige otros idiomas, o permite plugins en otros idiomas, en Ajustes › Fuentes.", "Kein aktiviertes Plugin bietet {wanted}. Wähle andere Sprachen oder lass Plugins in anderen Sprachen zu, unter Einstellungen › Quellen.", "لا توجد إضافة مفعّلة توفّر {wanted}. اختر لغات أخرى، أو اسمح بإضافات بلغات أخرى، من الإعدادات › المصادر.", "Nessuna estensione attiva offre {wanted}. Scegli altre lingue, o consenti estensioni in altre lingue, in Impostazioni › Fonti."]),
    ("download_failed", ["Download failed: {title}, episode {episode}", "Échec du téléchargement : {title}, épisode {episode}", "ダウンロード失敗：{title} 第{episode}話", "다운로드 실패: {title}, {episode}화", "下载失败：{title} 第 {episode} 集", "Falha no download: {title}, episódio {episode}", "Не удалось скачать: {title}, серия {episode}", "डाउनलोड विफल: {title}, एपिसोड {episode}", "Error en la descarga: {title}, episodio {episode}", "Download fehlgeschlagen: {title}, Folge {episode}", "فشل التنزيل: {title}، الحلقة {episode}", "Download non riuscito: {title}, episodio {episode}"]),
    ("db_open_failed", ["Anix could not open its database and has to close.", "Anix n’a pas pu ouvrir sa base de données et doit se fermer.", "Anix はデータベースを開けなかったため、終了します。", "Anix가 데이터베이스를 열지 못해 종료해야 해요.", "Anix 无法打开其数据库，必须关闭。", "O Anix não conseguiu abrir o banco de dados e precisa fechar.", "Anix не удалось открыть свою базу данных, и он будет закрыт.", "Anix अपना डेटाबेस नहीं खोल सका और उसे बंद होना होगा।", "Anix no pudo abrir su base de datos y tiene que cerrarse.", "Anix konnte seine Datenbank nicht öffnen und muss beendet werden.", "تعذّر على Anix فتح قاعدة بياناته ويجب إغلاقه.", "Anix non è riuscito ad aprire il suo database e deve chiudersi."]),
    ("picker_download_folder", ["Choose the download folder", "Choisir le dossier de téléchargement", "ダウンロード先のフォルダーを選択", "다운로드 폴더 선택", "选择下载文件夹", "Escolher a pasta de downloads", "Выберите папку для загрузок", "डाउनलोड फ़ोल्डर चुनें", "Elegir la carpeta de descargas", "Download-Ordner wählen", "اختر مجلد التنزيلات", "Scegli la cartella dei download"]),
    ("picker_episode_files", ["Choose episode files", "Choisir les fichiers des épisodes", "エピソードのファイルを選択", "에피소드 파일 선택", "选择剧集文件", "Escolher arquivos de episódios", "Выберите файлы серий", "एपिसोड फ़ाइलें चुनें", "Elegir archivos de episodios", "Folgendateien wählen", "اختر ملفات الحلقات", "Scegli i file degli episodi"]),
    ("picker_episode_folder", ["Choose the folder with the episodes", "Choisir le dossier des épisodes", "エピソードが入ったフォルダーを選択", "에피소드가 있는 폴더 선택", "选择存放剧集的文件夹", "Escolher a pasta com os episódios", "Выберите папку с сериями", "एपिसोड वाला फ़ोल्डर चुनें", "Elegir la carpeta con los episodios", "Ordner mit den Folgen wählen", "اختر المجلد الذي يحوي الحلقات", "Scegli la cartella con gli episodi"]),
    ("picker_video", ["Video", "Vidéo", "動画", "동영상", "视频", "Vídeo", "Видео", "वीडियो", "Vídeo", "Video", "فيديو", "Video"]),
    ("picker_save_backup", ["Save backup", "Enregistrer la sauvegarde", "バックアップを保存", "백업 저장", "保存备份", "Salvar backup", "Сохранить резервную копию", "बैकअप सहेजें", "Guardar copia de seguridad", "Sicherung speichern", "حفظ النسخة الاحتياطية", "Salva backup"]),
    ("picker_restore_backup", ["Restore a backup", "Restaurer une sauvegarde", "バックアップから復元", "백업 복원", "恢复备份", "Restaurar um backup", "Восстановить из резервной копии", "बैकअप बहाल करें", "Restaurar una copia de seguridad", "Sicherung wiederherstellen", "استعادة نسخة احتياطية", "Ripristina un backup"]),
    ("picker_backup_files", ["Anix backup", "Sauvegarde Anix", "Anix バックアップ", "Anix 백업", "Anix 备份", "Backup do Anix", "Резервная копия Anix", "Anix बैकअप", "Copia de seguridad de Anix", "Anix-Sicherung", "نسخة Anix الاحتياطية", "Backup di Anix"]),
    ("picker_save_report", ["Save diagnostics report", "Enregistrer le rapport de diagnostic", "診断レポートを保存", "진단 보고서 저장", "保存诊断报告", "Salvar relatório de diagnóstico", "Сохранить диагностический отчёт", "डायग्नोस्टिक रिपोर्ट सहेजें", "Guardar informe de diagnóstico", "Diagnosebericht speichern", "حفظ تقرير التشخيص", "Salva rapporto di diagnostica"]),
    ("picker_text_files", ["Text", "Texte", "テキスト", "텍스트", "文本", "Texto", "Текст", "टेक्स्ट", "Texto", "Text", "نص", "Testo"]),
];

/// Language names for `language_name`, in the columns of `LANGUAGES` without English (that
/// one comes from `providers::languages`). As written inside a sentence.
const LANGUAGE_NAMES: &[(&str, [&str; 11])] = &[
    ("en", ["anglais", "英語", "영어", "英语", "inglês", "английский", "अंग्रेज़ी", "inglés", "Englisch", "الإنجليزية", "inglese"]),
    ("ja", ["japonais", "日本語", "일본어", "日语", "japonês", "японский", "जापानी", "japonés", "Japanisch", "اليابانية", "giapponese"]),
    ("es", ["espagnol", "スペイン語", "스페인어", "西班牙语", "espanhol", "испанский", "स्पेनिश", "español", "Spanisch", "الإسبانية", "spagnolo"]),
    ("pt", ["portugais", "ポルトガル語", "포르투갈어", "葡萄牙语", "português", "португальский", "पुर्तगाली", "portugués", "Portugiesisch", "البرتغالية", "portoghese"]),
    ("fr", ["français", "フランス語", "프랑스어", "法语", "francês", "французский", "फ़्रेंच", "francés", "Französisch", "الفرنسية", "francese"]),
    ("de", ["allemand", "ドイツ語", "독일어", "德语", "alemão", "немецкий", "जर्मन", "alemán", "Deutsch", "الألمانية", "tedesco"]),
    ("it", ["italien", "イタリア語", "이탈리아어", "意大利语", "italiano", "итальянский", "इतालवी", "italiano", "Italienisch", "الإيطالية", "italiano"]),
    ("ru", ["russe", "ロシア語", "러시아어", "俄语", "russo", "русский", "रूसी", "ruso", "Russisch", "الروسية", "russo"]),
    ("ar", ["arabe", "アラビア語", "아랍어", "阿拉伯语", "árabe", "арабский", "अरबी", "árabe", "Arabisch", "العربية", "arabo"]),
    ("hi", ["hindi", "ヒンディー語", "힌디어", "印地语", "hindi", "хинди", "हिन्दी", "hindi", "Hindi", "الهندية", "hindi"]),
    ("id", ["indonésien", "インドネシア語", "인도네시아어", "印度尼西亚语", "indonésio", "индонезийский", "इंडोनेशियाई", "indonesio", "Indonesisch", "الإندونيسية", "indonesiano"]),
    ("ms", ["malais", "マレー語", "말레이어", "马来语", "malaio", "малайский", "मलय", "malayo", "Malaiisch", "الماليزية", "malese"]),
    ("th", ["thaï", "タイ語", "태국어", "泰语", "tailandês", "тайский", "थाई", "tailandés", "Thailändisch", "التايلاندية", "thailandese"]),
    ("vi", ["vietnamien", "ベトナム語", "베트남어", "越南语", "vietnamita", "вьетнамский", "वियतनामी", "vietnamita", "Vietnamesisch", "الفيتنامية", "vietnamita"]),
    ("tr", ["turc", "トルコ語", "튀르키예어", "土耳其语", "turco", "турецкий", "तुर्की", "turco", "Türkisch", "التركية", "turco"]),
    ("pl", ["polonais", "ポーランド語", "폴란드어", "波兰语", "polonês", "польский", "पोलिश", "polaco", "Polnisch", "البولندية", "polacco"]),
    ("nl", ["néerlandais", "オランダ語", "네덜란드어", "荷兰语", "holandês", "нидерландский", "डच", "neerlandés", "Niederländisch", "الهولندية", "olandese"]),
    ("sv", ["suédois", "スウェーデン語", "스웨덴어", "瑞典语", "sueco", "шведский", "स्वीडिश", "sueco", "Schwedisch", "السويدية", "svedese"]),
    ("no", ["norvégien", "ノルウェー語", "노르웨이어", "挪威语", "norueguês", "норвежский", "नॉर्वेजियाई", "noruego", "Norwegisch", "النرويجية", "norvegese"]),
    ("da", ["danois", "デンマーク語", "덴마크어", "丹麦语", "dinamarquês", "датский", "डेनिश", "danés", "Dänisch", "الدانمركية", "danese"]),
    ("fi", ["finnois", "フィンランド語", "핀란드어", "芬兰语", "finlandês", "финский", "फ़िनिश", "finés", "Finnisch", "الفنلندية", "finlandese"]),
    ("cs", ["tchèque", "チェコ語", "체코어", "捷克语", "tcheco", "чешский", "चेक", "checo", "Tschechisch", "التشيكية", "ceco"]),
    ("sk", ["slovaque", "スロバキア語", "슬로바키아어", "斯洛伐克语", "eslovaco", "словацкий", "स्लोवाक", "eslovaco", "Slowakisch", "السلوفاكية", "slovacco"]),
    ("hu", ["hongrois", "ハンガリー語", "헝가리어", "匈牙利语", "húngaro", "венгерский", "हंगेरियाई", "húngaro", "Ungarisch", "الهنغارية", "ungherese"]),
    ("ro", ["roumain", "ルーマニア語", "루마니아어", "罗马尼亚语", "romeno", "румынский", "रोमानियाई", "rumano", "Rumänisch", "الرومانية", "rumeno"]),
    ("bg", ["bulgare", "ブルガリア語", "불가리아어", "保加利亚语", "búlgaro", "болгарский", "बुल्गारियाई", "búlgaro", "Bulgarisch", "البلغارية", "bulgaro"]),
    ("uk", ["ukrainien", "ウクライナ語", "우크라이나어", "乌克兰语", "ucraniano", "украинский", "यूक्रेनियाई", "ucraniano", "Ukrainisch", "الأوكرانية", "ucraino"]),
    ("el", ["grec", "ギリシャ語", "그리스어", "希腊语", "grego", "греческий", "यूनानी", "griego", "Griechisch", "اليونانية", "greco"]),
    ("he", ["hébreu", "ヘブライ語", "히브리어", "希伯来语", "hebraico", "иврит", "हिब्रू", "hebreo", "Hebräisch", "العبرية", "ebraico"]),
    ("fa", ["persan", "ペルシア語", "페르시아어", "波斯语", "persa", "персидский", "फ़ारसी", "persa", "Persisch", "الفارسية", "persiano"]),
    ("ur", ["ourdou", "ウルドゥー語", "우르두어", "乌尔都语", "urdu", "урду", "उर्दू", "urdu", "Urdu", "الأوردية", "urdu"]),
    ("bn", ["bengali", "ベンガル語", "벵골어", "孟加拉语", "bengali", "бенгальский", "बंगाली", "bengalí", "Bengalisch", "البنغالية", "bengalese"]),
    ("ta", ["tamoul", "タミル語", "타밀어", "泰米尔语", "tâmil", "тамильский", "तमिल", "tamil", "Tamil", "التاميلية", "tamil"]),
    ("te", ["télougou", "テルグ語", "텔루구어", "泰卢固语", "télugo", "телугу", "तेलुगू", "telugu", "Telugu", "التيلوغوية", "telugu"]),
    ("ml", ["malayalam", "マラヤーラム語", "말라얄람어", "马拉雅拉姆语", "malaiala", "малаялам", "मलयालम", "malayálam", "Malayalam", "المالايالامية", "malayalam"]),
    ("kn", ["kannada", "カンナダ語", "칸나다어", "卡纳达语", "canarim", "каннада", "कन्नड़", "canarés", "Kannada", "الكانادا", "kannada"]),
    ("mr", ["marathi", "マラーティー語", "마라티어", "马拉地语", "marati", "маратхи", "मराठी", "maratí", "Marathi", "الماراثية", "marathi"]),
    ("zh", ["chinois", "中国語", "중국어", "中文", "chinês", "китайский", "चीनी", "chino", "Chinesisch", "الصينية", "cinese"]),
    ("ko", ["coréen", "韓国語", "한국어", "韩语", "coreano", "корейский", "कोरियाई", "coreano", "Koreanisch", "الكورية", "coreano"]),
    ("tl", ["filipino", "フィリピノ語", "필리핀어", "菲律宾语", "filipino", "филиппинский", "फ़िलिपीनो", "filipino", "Filipino", "الفلبينية", "filippino"]),
    ("hr", ["croate", "クロアチア語", "크로아티아어", "克罗地亚语", "croata", "хорватский", "क्रोएशियाई", "croata", "Kroatisch", "الكرواتية", "croato"]),
    ("sr", ["serbe", "セルビア語", "세르비아어", "塞尔维亚语", "sérvio", "сербский", "सर्बियाई", "serbio", "Serbisch", "الصربية", "serbo"]),
    ("ca", ["catalan", "カタロニア語", "카탈로니아어", "加泰罗尼亚语", "catalão", "каталанский", "कातालान", "catalán", "Katalanisch", "الكتالانية", "catalano"]),
    ("lt", ["lituanien", "リトアニア語", "리투아니아어", "立陶宛语", "lituano", "литовский", "लिथुआनियाई", "lituano", "Litauisch", "الليتوانية", "lituano"]),
    ("lv", ["letton", "ラトビア語", "라트비아어", "拉脱维亚语", "letão", "латышский", "लातवियाई", "letón", "Lettisch", "اللاتفية", "lettone"]),
    ("et", ["estonien", "エストニア語", "에스토니아어", "爱沙尼亚语", "estoniano", "эстонский", "एस्टोनियाई", "estonio", "Estnisch", "الإستونية", "estone"]),
];

fn column(lang: &str) -> usize {
    LANGUAGES.iter().position(|code| *code == lang).unwrap_or(0)
}

/// The supported language for an `app_language` value: a known code as saved, otherwise
/// ("system", missing) the first one the locale variables name, else English.
pub fn resolve(setting: Option<&str>, system: &[Option<String>]) -> &'static str {
    let known = |code: &str| LANGUAGES.iter().find(|c| **c == code).copied();
    if let Some(code) = setting.and_then(|s| known(s.trim())) {
        return code;
    }
    for value in system.iter().flatten() {
        let primary = value.split(|c| c == '_' || c == '-' || c == '.' || c == '@').next().unwrap_or("");
        if let Some(code) = known(&primary.to_ascii_lowercase()) {
            return code;
        }
    }
    "en"
}

fn system_locale() -> Vec<Option<String>> {
    system_languages(|name| std::env::var(name).ok())
}

/// The locale values to try, best first, from the environment variable reader `var`.
pub fn system_languages(var: impl Fn(&str) -> Option<String>) -> Vec<Option<String>> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    // gettext's rule: LANGUAGE ("fr:en") comes first, unless the locale itself is C/POSIX.
    let locale = set("LC_ALL").or_else(|| set("LC_MESSAGES")).or_else(|| set("LANG"));
    let mut list: Vec<Option<String>> = match locale.as_deref() {
        Some(v) if v == "C" || v == "POSIX" || v.starts_with("C.") => return Vec::new(),
        Some(_) => set("LANGUAGE").map(|v| v.split(':').map(|s| Some(s.to_string())).collect()).unwrap_or_default(),
        None => Vec::new(),
    };
    list.push(locale);
    list
}

fn read_setting() -> Option<String> {
    let dir = crate::db::get_config_dir().ok()?;
    let text = std::fs::read_to_string(dir.join("settings.json")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get("app_language").and_then(Value::as_str).map(str::to_string)
}

static CACHED: Lazy<Mutex<Option<(Instant, &'static str)>>> = Lazy::new(|| Mutex::new(None));

/// The interface language now, re-read from settings.json at most every few seconds.
pub fn current() -> &'static str {
    let mut guard = CACHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((read_at, lang)) = *guard {
        if read_at.elapsed() < REFRESH_EVERY {
            return lang;
        }
    }
    let lang = resolve(read_setting().as_deref(), &system_locale());
    *guard = Some((Instant::now(), lang));
    lang
}

/// Drops the cached language, so the next `current()` reads settings.json again.
pub fn forget() {
    *CACHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// The text for `key` in `lang` (English when `lang` is unknown; the key itself if missing).
pub fn text<'a>(lang: &str, key: &'a str) -> &'a str {
    TEXTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, texts)| texts[column(lang)])
        .unwrap_or(key)
}

/// `text(lang, key)` with each `{name}` replaced by its value.
pub fn fill(lang: &str, key: &str, values: &[(&str, &str)]) -> String {
    let mut out = text(lang, key).to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// A language code's name in `lang`, as it reads inside a sentence.
pub fn language_name(lang: &str, code: &str) -> String {
    let col = column(lang);
    if col == 0 {
        return crate::providers::languages::language_name(code);
    }
    LANGUAGE_NAMES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(code))
        .map(|(_, names)| names[col - 1].to_string())
        .unwrap_or_else(|| crate::providers::languages::language_name(code))
}

/// Language names joined as a list: "English", "English or Spanish", "English, Spanish or Hindi".
pub fn language_list(lang: &str, codes: &[String]) -> String {
    let names: Vec<String> = codes.iter().map(|code| language_name(lang, code)).collect();
    match names.len() {
        0 => text(lang, "your_languages").to_string(),
        1 => names[0].clone(),
        n => fill(
            lang,
            "or",
            &[("list", &names[..n - 1].join(text(lang, "list_separator"))), ("last", &names[n - 1])],
        ),
    }
}

/// Short weekday name, 0 = Sunday.
pub fn weekday(lang: &str, day: usize) -> &'static str {
    const KEYS: [&str; 7] = ["day0", "day1", "day2", "day3", "day4", "day5", "day6"];
    text(lang, KEYS[day % 7])
}
