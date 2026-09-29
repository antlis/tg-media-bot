"""Custom extractor plugins: user-supplied resolvers for pages yt-dlp can't (or
shouldn't) handle out of the box.

A plugin is a plain ``.py`` file in the plugin directory (``PLUGIN_DIR``) that
exposes two module-level callables::

    def match(url: str) -> bool:
        '''Return True for URLs this plugin knows how to resolve.'''

    async def resolve(url: str):   # may also be a plain (sync) def
        '''Turn the page URL into something yt-dlp can download.'''

``resolve`` may return:
    * a media URL ``str``,
    * a ``(media_url, referer)`` tuple,
    * a :class:`ResolveResult` (media URL + referer + extra headers), or
    * ``None`` to let other plugins / yt-dlp / the browser fallback try.

The resolved media URL is handed to yt-dlp exactly like the headless-browser
fallback's capture, so format selection, recode and upload all still apply.
Plugins are matched in load order (alphabetical by filename); the first whose
``match`` is true and whose ``resolve`` yields a URL wins. A plugin that raises
is logged and skipped — it can never crash a download.

Plugins are an opt-in convenience: point ``PLUGIN_DIR`` at a directory (in Docker
the mounted ``./plugins``). With none present nothing changes.
"""

import importlib.util
import inspect
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, List, Optional

from ..utils.logger import get_logger

logger = get_logger()


@dataclass
class ResolveResult:
    """What a plugin's ``resolve`` hands back for yt-dlp to download."""

    media_url: str
    referer: Optional[str] = None
    headers: dict = field(default_factory=dict)


@dataclass
class _Plugin:
    name: str
    match: Callable[[str], bool]
    resolve: Callable


def load_plugins(plugin_dir: str) -> List[_Plugin]:
    """Import every ``*.py`` in *plugin_dir* that exposes ``match`` + ``resolve``.

    Files whose name starts with ``_`` are skipped (helpers). A file that fails
    to import is logged and ignored rather than taking the bot down.
    """
    plugins: List[_Plugin] = []
    directory = Path(plugin_dir)
    if not directory.is_dir():
        return plugins
    for path in sorted(directory.glob("*.py")):
        if path.name.startswith("_"):
            continue
        try:
            spec = importlib.util.spec_from_file_location(f"tgmb_plugin_{path.stem}", path)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)  # type: ignore[union-attr]
            match = getattr(module, "match", None)
            resolve = getattr(module, "resolve", None)
            if not callable(match) or not callable(resolve):
                logger.warning("Plugin skipped: no match()/resolve()", plugin=path.name)
                continue
            plugins.append(_Plugin(name=path.stem, match=match, resolve=resolve))
            logger.info("Loaded extractor plugin", plugin=path.stem)
        except Exception as exc:
            logger.warning("Plugin failed to load", plugin=path.name, error=str(exc))
    return plugins


def _normalize(result, page_url: str) -> Optional[ResolveResult]:
    """Coerce a plugin's return value into a ResolveResult (or None)."""
    if result is None:
        return None
    if isinstance(result, ResolveResult):
        if not result.referer:
            result.referer = page_url
        return result
    if isinstance(result, str):
        return ResolveResult(media_url=result, referer=page_url)
    if isinstance(result, (tuple, list)) and result:
        media_url = result[0]
        referer = result[1] if len(result) > 1 and result[1] else page_url
        return ResolveResult(media_url=media_url, referer=referer)
    return None


async def resolve_with_plugins(url: str, plugins: List[_Plugin]) -> Optional[ResolveResult]:
    """First matching plugin that yields a media URL wins; broken plugins skipped."""
    for plugin in plugins:
        try:
            if not plugin.match(url):
                continue
        except Exception as exc:
            logger.warning("Plugin match() raised", plugin=plugin.name, error=str(exc))
            continue
        try:
            logger.info("Trying extractor plugin", plugin=plugin.name)
            out = plugin.resolve(url)
            if inspect.isawaitable(out):
                out = await out
            resolved = _normalize(out, url)
            if resolved and resolved.media_url:
                logger.info("Extractor plugin resolved a media URL", plugin=plugin.name)
                return resolved
        except Exception as exc:
            logger.warning("Plugin resolve() raised", plugin=plugin.name, error=str(exc))
    return None
