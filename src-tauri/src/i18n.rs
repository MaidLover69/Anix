//! Text the backend shows by itself — desktop notifications, the tray menu, release-check
//! and reminder lines — in the interface language (`app_language` in settings.json, else
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
