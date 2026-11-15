use super::Strings;

pub static STRINGS: Strings = Strings {
    welcome: "👋 <b>¡Hola!</b> Envíame enlaces a vídeos o canciones: te preguntaré la calidad y te mandaré los archivos aquí.\n\n/help — comandos\n/lang — idioma",
    help: "<b>Descargador de medios</b>\n\n\
Envía enlaces (puedes poner varios en un mensaje). Te preguntaré la calidad, los pondré en tu cola y te enviaré los archivos uno a uno.\n\n\
<b>Comandos</b>\n\
/quality — calidad por defecto (o preguntar cada vez)\n\
/status — tu cola\n\
/cancel &lt;id&gt; — cancelar una descarga, /cancel all — vaciar la cola\n\
/minimal on|off — sin mensajes de estado ni descripciones\n\
/topic lock|unlock|status — limitar el bot a un tema del foro\n\
/lang — idioma\n\n\
<b>Sitios</b>\n\
YouTube, TikTok, Instagram, X/Twitter, Reddit, Vimeo, Twitch, SoundCloud y más de 1000 (yt-dlp).",
    help_limits: "<b>Límites</b>\nArchivo de hasta {size} · vídeo de hasta {height}p",
    help_limit_duration: " · duración de hasta {dur}",
    lang_set: "✅ Idioma: {lang}",
    nudge: "👋 Envíame un enlace para descargar.\n/help — comandos",
    not_authorized: "⛔ No tienes permiso para usar este bot.",
    cleared: "🧹 Historial de mensajes borrado.",

    quality_current: "🎚 Calidad para enlaces nuevos: <b>{q}</b>\nElige qué usar a partir de ahora:",
    quality_set: "✅ Los enlaces nuevos se descargarán así: <b>{q}</b>. Cámbialo con /quality.",
    word_ask: "preguntar cada vez",

    cancel_usage: "Uso: /cancel &lt;id&gt; o /cancel all\nLos id están en /status.",
    cancelled: "🚫 Cancelado.",
    cancelled_n: "🚫 Descargas canceladas: {n}",
    cancel_fail: "❌ No hay ninguna descarga activa con el id <code>{id}</code>.",
    cancel_denied: "Solo quien inició esta descarga puede cancelarla.",
    status_empty: "Tu cola está vacía.",
    status_header: "📊 <b>Tu cola</b> ({n}/{max})",
    status_more: "…y {n} más",

    minimal_status: "El modo mínimo está <b>{state}</b> en este chat.\nUso: /minimal on|off",
    minimal_on: "🤫 Modo mínimo activado: sin mensajes de estado ni descripciones.",
    minimal_off: "✅ Modo mínimo desactivado.",
    word_on: "activado",
    word_off: "desactivado",

    topic_groups_only: "El bloqueo de tema solo funciona en grupos con temas.",
    topic_lock_outside: "Envía /topic lock dentro del tema en el que quieres que me quede.",
    topic_locked: "🔒 A partir de ahora solo respondo en este tema. /topic unlock para deshacerlo.",
    topic_unlocked: "🔓 Bloqueo de tema eliminado: vuelvo a responder en todos los temas.",
    topic_none: "Este grupo no tiene bloqueo de tema.",
    topic_status: "🔒 Limitado al tema {id}.",
    topic_usage: "Uso: /topic lock|unlock|status",

    formats_usage: "Uso: /formats &lt;enlace&gt;",
    ask_quality: "🎚 ¿En qué calidad lo descargo?",
    ask_quality_many: "🎚 ¿En qué calidad los descargo? Enlaces: {n}",
    queued_n: "✅ {choice} · en cola: {n}",
    picker_expired: "Este menú ha caducado: envía el enlace de nuevo.",
    not_yours: "Estos botones son para quien envió los enlaces.",
    btn_max: "🎬 Máxima",
    btn_audio: "🎵 Audio",
    btn_ask: "❓ Preguntar",
    btn_cancel: "✖ Cancelar",
    btn_cancel_all: "✖ Cancelar todo",

    invalid_url: "❌ Eso no parece un enlace válido.",
    private_url: "⛔ No se permiten enlaces a direcciones locales o privadas.",
    user_queue_full: "⏳ Tu cola está llena: {n} de {max}. Espera a que terminen algunas descargas o vacíala con /cancel all.",
    queue_partial: "⚠️ Se añadieron {n}; otros {skipped} no cupieron (tu cola admite {max}).",
    queue_full: "⏳ La cola del bot está llena ahora mismo: inténtalo más tarde.",
    duplicate: "⏳ Este enlace ya está en la cola.",

    st_queued: "⏳ En cola",
    st_waiting: "⏳ Esperando un hueco libre",
    st_probing: "🔎 Comprobando el enlace",
    st_downloading: "📥 Descargando",
    st_processing: "🔄 Procesando",
    st_transcoding: "🎞 Convirtiendo",
    st_uploading: "📤 Enviando",
    st_title_pending: "obteniendo título…",
    st_resuming: "🔄 Reanudando descarga tras reinicio del bot…",

    restarted_notice: "⚠️ El bot se reinició. Por favor, envía el enlace de nuevo.",
    err_title: "❌ <b>No se pudo descargar</b>",
    e_age: "🔞 Restringido por edad. El dueño del bot debe configurar COOKIES_FILE con una cuenta con sesión iniciada.",
    e_bot: "🤖 El sitio pide iniciar sesión para comprobar que no eres un bot. El dueño del bot debe configurar COOKIES_FILE.",
    e_private: "🔒 Este vídeo es privado.",
    e_members: "🔒 Contenido solo para miembros.",
    e_login: "🔑 Hace falta iniciar sesión. El dueño del bot debe configurar COOKIES_FILE con una cuenta con sesión iniciada.",
    e_format: "🎚 Esa calidad no está disponible: elige otra.",
    e_unavailable: "🚫 El vídeo no está disponible (eliminado o no público).",
    e_unsupported: "🤔 Este enlace no es compatible.",
    e_no_media: "🤔 No se encontró vídeo ni audio en este enlace.",
    e_404: "🚫 No encontrado (404): puede que el enlace esté roto.",
    e_403: "⛔ Acceso denegado (403): puede que hagan falta cookies u otra región.",
    e_rate: "⏳ El sitio está limitando las peticiones: inténtalo más tarde.",
    e_network: "🌐 No se pudo conectar con el sitio: inténtalo dentro de un rato.",
    e_geo: "🌍 No disponible en la región del servidor (el dueño del bot puede configurar PROXY_URL).",
    e_drm: "🔐 Este contenido está protegido con DRM y no se puede descargar.",
    e_live: "🔴 Las emisiones en directo no son compatibles: inténtalo cuando termine la emisión.",
    e_too_long: "⏱ Demasiado largo: {dur} (límite {max}).",
    e_too_large: "📦 Demasiado grande: unos {size} (límite {max}).\nPrueba una calidad más baja o solo audio.",
    e_no_space: "💾 Al servidor le queda poco espacio en disco: inténtalo más tarde.",
    e_timeout: "⏱ Se agotó el tiempo tras {min} min.",
    e_probe_timeout: "⏱ El sitio web tardó demasiado en responder: posible problema de red o conexión.",
    e_upload: "❌ Telegram no aceptó el archivo ({size}).",
    e_upload_big: "📦 El archivo pesa {size}: supera el límite de envío de {max}.",
    e_internal: "❌ Error interno. Los detalles están en el registro del bot.",

    c_start: "Iniciar el bot",
    c_help: "Ayuda y sitios compatibles",
    c_quality: "Calidad por defecto",
    c_status: "Tu cola",
    c_cancel: "Cancelar una descarga (o todas)",
    c_clear: "Borrar historial de mensajes: [cantidad]",
    c_minimal: "Interfaz mínima: on|off",
    c_topic: "Limitar a un tema: lock|unlock|status",
    c_lang: "Idioma",
};
