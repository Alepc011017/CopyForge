"""Tests de la lógica pura de la extensión de Nautilus (sin `gi`).

Ejecutar desde la raíz del repo:

    python3 -m unittest discover integrations/nautilus/tests
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(
    0, os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)

import copyforge_ext_logic as logic  # noqa: E402


class ParseClipboardTest(unittest.TestCase):
    def test_copy(self):
        action, paths = logic.parse_clipboard("copy\nfile:///a\nfile:///b\n")
        self.assertEqual(action, "copy")
        self.assertEqual(paths, ["/a", "/b"])

    def test_cut(self):
        action, paths = logic.parse_clipboard("cut\nfile:///a\n")
        self.assertEqual(action, "cut")
        self.assertEqual(paths, ["/a"])

    def test_comentarios_y_lineas_vacias(self):
        data = "cut\n# comentario\n\nfile:///a\n\n# otro\nfile:///b\n"
        self.assertEqual(logic.parse_clipboard(data), ("cut", ["/a", "/b"]))

    def test_url_encoded(self):
        action, paths = logic.parse_clipboard(
            "cut\nfile:///home/u/mis%20fotos/a%20b.png\n"
        )
        self.assertEqual(action, "cut")
        self.assertEqual(paths, ["/home/u/mis fotos/a b.png"])

    def test_sin_cabecera_asume_copy(self):
        self.assertEqual(logic.parse_clipboard("file:///a\n"), ("copy", ["/a"]))

    def test_vacio(self):
        self.assertEqual(logic.parse_clipboard(""), ("copy", []))

    def test_ignora_lineas_que_no_son_file(self):
        data = "copy\nhttps://ejemplo/x\nfile:///a\n"
        self.assertEqual(logic.parse_clipboard(data), ("copy", ["/a"]))


class FilterSourcesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = self.tmp.name
        self.dst = os.path.join(self.root, "dst")
        self.src = os.path.join(self.root, "src")
        self.inside_dst = os.path.join(self.dst, "sub")
        self.inside_src = os.path.join(self.src, "sub")
        for d in (self.dst, self.src, self.inside_dst, self.inside_src):
            os.makedirs(d, exist_ok=True)

    def test_todo_ok(self):
        valid, skipped = logic.filter_sources([self.src], self.dst)
        self.assertEqual(valid, [self.src])
        self.assertEqual(skipped, [])

    def test_src_igual_dst(self):
        valid, skipped = logic.filter_sources([self.dst], self.dst)
        self.assertEqual(valid, [])
        self.assertEqual(skipped, [(self.dst, "es el destino")])

    def test_src_dentro_de_dst(self):
        valid, skipped = logic.filter_sources([self.inside_dst], self.dst)
        self.assertEqual(valid, [])
        self.assertEqual(skipped, [(self.inside_dst, "está dentro del destino")])

    def test_dst_dentro_de_src(self):
        valid, skipped = logic.filter_sources([self.src], self.inside_src)
        self.assertEqual(valid, [])
        self.assertEqual(skipped, [(self.src, "contiene al destino")])

    def test_inexistente(self):
        missing = os.path.join(self.root, "no-existe")
        valid, skipped = logic.filter_sources([missing], self.dst)
        self.assertEqual(valid, [])
        self.assertEqual(skipped, [(missing, "no existe")])

    def test_mezcla(self):
        valid, skipped = logic.filter_sources([self.src, self.dst], self.dst)
        self.assertEqual(valid, [self.src])
        self.assertEqual(len(skipped), 1)


class GuiBinTest(unittest.TestCase):
    def test_env_var_tiene_prioridad(self):
        env = {"COPYFORGE_GUI_BIN": "/opt/cf/bin"}
        self.assertEqual(
            logic.gui_bin(env=env, which=lambda _n: "/no/deberia/usarse"),
            "/opt/cf/bin",
        )

    def test_env_var_vacia_usa_which(self):
        env = {"COPYFORGE_GUI_BIN": "   "}
        found = logic.gui_bin(env=env, which=lambda n: "/usr/bin/" + n)
        self.assertEqual(found, "/usr/bin/copyforge-gui-tauri")

    def test_orden_de_busqueda(self):
        calls = []

        def which(name):
            calls.append(name)
            return "/x/" + name if name == "copyforge" else None

        self.assertEqual(logic.gui_bin(env={}, which=which), "/x/copyforge")
        self.assertEqual(calls, ["copyforge-gui-tauri", "copyforge"])

    def test_sin_binario(self):
        self.assertIsNone(logic.gui_bin(env={}, which=lambda _n: None))


class ClipboardCommandsTest(unittest.TestCase):
    def test_lee_gnome_copied_files_primero(self):
        cmds = logic.clipboard_read_commands(which=lambda _n: "/usr/bin/tool")
        self.assertTrue(cmds)
        self.assertIn(logic.GNOME_COPIED_FILES, cmds[0])

    def test_escribe_con_gnome_copied_files(self):
        cmds = logic.clipboard_write_commands(which=lambda _n: "/usr/bin/tool")
        self.assertTrue(cmds)
        self.assertIn(logic.GNOME_COPIED_FILES, cmds[0])

    def test_sin_herramientas(self):
        self.assertEqual(logic.clipboard_read_commands(which=lambda _n: None), [])
        self.assertEqual(logic.clipboard_write_commands(which=lambda _n: None), [])

    def test_payload_ida_y_vuelta(self):
        payload = logic.clipboard_payload("cut", ["/a b/c", "/d"])
        self.assertEqual(
            logic.parse_clipboard(payload), ("cut", ["/a b/c", "/d"])
        )


if __name__ == "__main__":
    unittest.main()
