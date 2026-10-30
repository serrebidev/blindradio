# BlindRadio app module: makes NVDA+End find the status bar.
#
# BlindRadio's status bar is a standard wxWidgets status bar. It sits
# in the object tree with the right role and its full text, but NVDA's
# fallback lookup (the object at the bottom edge of the foreground
# window, then walk up the ancestry) does not land on it, so NVDA+End
# answers "No status line found". This module takes NVDA's documented
# first path instead: the app module hands NVDA the status bar object
# directly, found by walking the foreground window's tree — the same
# approach NVDA's own VS Code app module uses.

from collections import deque

import api
import appModuleHandler
import controlTypes


class AppModule(appModuleHandler.AppModule):
	def _get_statusBar(self):
		foreground = api.getForegroundObject()
		if foreground is None:
			raise NotImplementedError
		found = self._search(foreground)
		if found is None:
			raise NotImplementedError
		return found

	@staticmethod
	def _search(root):
		seen = set()
		queue = deque([root])
		while queue:
			obj = queue.popleft()
			if obj in seen:
				continue
			seen.add(obj)
			try:
				if obj.role == controlTypes.Role.STATUSBAR:
					return obj
			except Exception:
				pass
			try:
				queue.extend(obj.children)
			except Exception:
				pass
		return None
