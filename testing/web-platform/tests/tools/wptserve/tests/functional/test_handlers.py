import json
import os
import sys
import unittest
import uuid

import pytest
from urllib.error import HTTPError

wptserve = pytest.importorskip("wptserve")
from .base import TestUsingServer, TestUsingH2Server, doc_root
from .base import TestWrapperHandlerUsingServer

from serve import serve


class TestFileHandler(TestUsingServer):
    def test_GET(self):
        resp = self.request("/document.txt")
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/plain", resp.info()["Content-Type"])
        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        self.assertEqual(expected, resp.read())

    def test_headers(self):
        resp = self.request("/with_headers.txt")
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/html", resp.info()["Content-Type"])
        self.assertEqual("PASS", resp.info()["Custom-Header"])
        # This will fail if it isn't a valid uuid
        uuid.UUID(resp.info()["Another-Header"])
        self.assertEqual(resp.info()["Same-Value-Header"], resp.info()["Another-Header"])
        self.assert_multiple_headers(resp, "Double-Header", ["PA", "SS"])


    def test_range(self):
        resp = self.request("/document.txt", headers={"Range":"bytes=10-19"})
        self.assertEqual(206, resp.getcode())
        data = resp.read()
        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        self.assertEqual(10, len(data))
        self.assertEqual("bytes 10-19/%i" % len(expected), resp.info()['Content-Range'])
        self.assertEqual("10", resp.info()['Content-Length'])
        self.assertEqual(expected[10:20], data)

    def test_range_no_end(self):
        resp = self.request("/document.txt", headers={"Range":"bytes=10-"})
        self.assertEqual(206, resp.getcode())
        data = resp.read()
        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        self.assertEqual(len(expected) - 10, len(data))
        self.assertEqual("bytes 10-%i/%i" % (len(expected) - 1, len(expected)), resp.info()['Content-Range'])
        self.assertEqual(expected[10:], data)

    def test_range_no_start(self):
        resp = self.request("/document.txt", headers={"Range":"bytes=-10"})
        self.assertEqual(206, resp.getcode())
        data = resp.read()
        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        self.assertEqual(10, len(data))
        self.assertEqual("bytes %i-%i/%i" % (len(expected) - 10, len(expected) - 1, len(expected)),
                         resp.info()['Content-Range'])
        self.assertEqual(expected[-10:], data)

    def test_multiple_ranges(self):
        resp = self.request("/document.txt", headers={"Range":"bytes=1-2,5-7,6-10"})
        self.assertEqual(206, resp.getcode())
        data = resp.read()
        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        self.assertTrue(resp.info()["Content-Type"].startswith("multipart/byteranges; boundary="))
        boundary = resp.info()["Content-Type"].split("boundary=")[1]
        parts = data.split(b"--" + boundary.encode("ascii"))
        self.assertEqual(b"\r\n", parts[0])
        self.assertEqual(b"--", parts[-1])
        expected_parts = [(b"1-2", expected[1:3]), (b"5-10", expected[5:11])]
        for expected_part, part in zip(expected_parts, parts[1:-1]):
            header_string, body = part.split(b"\r\n\r\n")
            headers = dict(item.split(b": ", 1) for item in header_string.split(b"\r\n") if item.strip())
            self.assertEqual(headers[b"Content-Type"], b"text/plain")
            self.assertEqual(headers[b"Content-Range"], b"bytes %s/%i" % (expected_part[0], len(expected)))
            self.assertEqual(expected_part[1] + b"\r\n", body)

    def test_range_invalid(self):
        with self.assertRaises(HTTPError) as cm:
            self.request("/document.txt", headers={"Range":"bytes=11-10"})
        self.assertEqual(cm.exception.code, 416)

        with open(os.path.join(doc_root, "document.txt"), 'rb') as f:
            expected = f.read()
        with self.assertRaises(HTTPError) as cm:
            self.request("/document.txt", headers={"Range":"bytes=%i-%i" % (len(expected), len(expected) + 10)})
        self.assertEqual(cm.exception.code, 416)

    def test_sub_config(self):
        resp = self.request("/sub.sub.txt")
        expected = b"localhost localhost %i" % self.server.port
        assert resp.read().rstrip() == expected

    def test_sub_headers(self):
        resp = self.request("/sub_headers.sub.txt", headers={"X-Test": "PASS"})
        expected = b"PASS"
        assert resp.read().rstrip() == expected

    def test_sub_params(self):
        resp = self.request("/sub_params.txt", query="plus+pct-20%20pct-3D%3D=PLUS+PCT-20%20PCT-3D%3D&pipe=sub")
        expected = b"PLUS PCT-20 PCT-3D="
        assert resp.read().rstrip() == expected


class TestFunctionHandler(TestUsingServer):
    def test_string_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return "test data"

        route = ("GET", "/test/test_string_rv", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(200, resp.getcode())
        self.assertEqual("9", resp.info()["Content-Length"])
        self.assertEqual(b"test data", resp.read())

    def test_tuple_1_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return ()

        route = ("GET", "/test/test_tuple_1_rv", handler)
        self.server.router.register(*route)

        with pytest.raises(HTTPError) as cm:
            self.request(route[1])

        assert cm.value.code == 500
        del cm

    def test_tuple_2_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return [("Content-Length", 4), ("test-header", "test-value")], "test data"

        route = ("GET", "/test/test_tuple_2_rv", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(200, resp.getcode())
        self.assertEqual("4", resp.info()["Content-Length"])
        self.assertEqual("test-value", resp.info()["test-header"])
        self.assertEqual(b"test", resp.read())

    def test_tuple_3_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return 202, [("test-header", "test-value")], "test data"

        route = ("GET", "/test/test_tuple_3_rv", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(202, resp.getcode())
        self.assertEqual("test-value", resp.info()["test-header"])
        self.assertEqual(b"test data", resp.read())

    def test_tuple_3_rv_1(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return (202, "Some Status"), [("test-header", "test-value")], "test data"

        route = ("GET", "/test/test_tuple_3_rv_1", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(202, resp.getcode())
        self.assertEqual("Some Status", resp.msg)
        self.assertEqual("test-value", resp.info()["test-header"])
        self.assertEqual(b"test data", resp.read())

    def test_tuple_4_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return 202, [("test-header", "test-value")], "test data", "garbage"

        route = ("GET", "/test/test_tuple_1_rv", handler)
        self.server.router.register(*route)

        with pytest.raises(HTTPError) as cm:
            self.request(route[1])

        assert cm.value.code == 500
        del cm

    def test_none_rv(self):
        @wptserve.handlers.handler
        def handler(request, response):
            return None

        route = ("GET", "/test/test_none_rv", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        assert resp.getcode() == 200
        assert "Content-Length" not in resp.info()
        assert resp.read() == b""


class TestJSONHandler(TestUsingServer):
    def test_json_0(self):
        @wptserve.handlers.json_handler
        def handler(request, response):
            return {"data": "test data"}

        route = ("GET", "/test/test_json_0", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(200, resp.getcode())
        self.assertEqual({"data": "test data"}, json.load(resp))

    def test_json_tuple_2(self):
        @wptserve.handlers.json_handler
        def handler(request, response):
            return [("Test-Header", "test-value")], {"data": "test data"}

        route = ("GET", "/test/test_json_tuple_2", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(200, resp.getcode())
        self.assertEqual("test-value", resp.info()["test-header"])
        self.assertEqual({"data": "test data"}, json.load(resp))

    def test_json_tuple_3(self):
        @wptserve.handlers.json_handler
        def handler(request, response):
            return (202, "Giraffe"), [("Test-Header", "test-value")], {"data": "test data"}

        route = ("GET", "/test/test_json_tuple_2", handler)
        self.server.router.register(*route)
        resp = self.request(route[1])
        self.assertEqual(202, resp.getcode())
        self.assertEqual("Giraffe", resp.msg)
        self.assertEqual("test-value", resp.info()["test-header"])
        self.assertEqual({"data": "test data"}, json.load(resp))


class TestPythonHandler(TestUsingServer):
    def test_string(self):
        resp = self.request("/test_string.py")
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/plain", resp.info()["Content-Type"])
        self.assertEqual(b"PASS", resp.read())

    def test_tuple_2(self):
        resp = self.request("/test_tuple_2.py")
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/html", resp.info()["Content-Type"])
        self.assertEqual("PASS", resp.info()["X-Test"])
        self.assertEqual(b"PASS", resp.read())

    def test_tuple_3(self):
        resp = self.request("/test_tuple_3.py")
        self.assertEqual(202, resp.getcode())
        self.assertEqual("Giraffe", resp.msg)
        self.assertEqual("text/html", resp.info()["Content-Type"])
        self.assertEqual("PASS", resp.info()["X-Test"])
        self.assertEqual(b"PASS", resp.read())

    def test_import(self):
        dir_name = os.path.join(doc_root, "subdir")
        assert dir_name not in sys.path
        assert "test_module" not in sys.modules
        resp = self.request("/subdir/import_handler.py")
        assert dir_name not in sys.path
        assert "test_module" not in sys.modules
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/plain", resp.info()["Content-Type"])
        self.assertEqual(b"PASS", resp.read())

    def test_no_main(self):
        with pytest.raises(HTTPError) as cm:
            self.request("/no_main.py")

        assert cm.value.code == 500
        del cm

    def test_invalid(self):
        with pytest.raises(HTTPError) as cm:
            self.request("/invalid.py")

        assert cm.value.code == 500
        del cm

    def test_missing(self):
        with pytest.raises(HTTPError) as cm:
            self.request("/missing.py")

        assert cm.value.code == 404
        del cm


class TestDirectoryHandler(TestUsingServer):
    def test_directory(self):
        resp = self.request("/")
        self.assertEqual(200, resp.getcode())
        self.assertEqual("text/html", resp.info()["Content-Type"])
        #Add a check that the response is actually sane

    def test_subdirectory_trailing_slash(self):
        resp = self.request("/subdir/")
        assert resp.getcode() == 200
        assert resp.info()["Content-Type"] == "text/html"

    def test_subdirectory_no_trailing_slash(self):
        # This seems to resolve the 301 transparently, so test for 200
        resp = self.request("/subdir")
        assert resp.getcode() == 200
        assert resp.info()["Content-Type"] == "text/html"


class TestAsIsHandler(TestUsingServer):
    def test_as_is(self):
        resp = self.request("/test.asis")
        self.assertEqual(202, resp.getcode())
        self.assertEqual("Giraffe", resp.msg)
        self.assertEqual("PASS", resp.info()["X-Test"])
        self.assertEqual(b"Content", resp.read())
        #Add a check that the response is actually sane


class TestH2Handler(TestUsingH2Server):
    def test_handle_headers(self):
        resp = self.client.get('/test_h2_headers.py')

        assert resp.status_code == 203
        assert resp.headers['test'] == 'passed'
        assert resp.content == b''

    def test_only_main(self):
        resp = self.client.get('/test_tuple_3.py')

        assert resp.status_code == 202
        assert resp.headers['Content-Type'] == 'text/html'
        assert resp.headers['X-Test'] == 'PASS'
        assert resp.content == b'PASS'

    def test_handle_data(self):
        resp = self.client.post('/test_h2_data.py', content=b'hello world!')

        assert resp.status_code == 200
        assert resp.content == b'HELLO WORLD!'

    def test_handle_headers_data(self):
        resp = self.client.post('/test_h2_headers_data.py', content=b'hello world!')

        assert resp.status_code == 203
        assert resp.headers['test'] == 'passed'
        assert resp.content == b'HELLO WORLD!'

    def test_no_main_or_handlers(self):
        resp = self.client.get('/no_main.py')

        assert resp.status_code == 500
        assert "No main function or handlers in script " in json.loads(resp.content)["error"]["message"]

    def test_not_found(self):
        resp = self.client.get('/no_exist.py')

        assert resp.status_code == 404

    def test_requesting_multiple_resources(self):
        # 1st .py resource
        resp = self.client.get('/test_h2_headers.py')

        assert resp.status_code == 203
        assert resp.headers['test'] == 'passed'
        assert resp.content == b''

        # 2nd .py resource
        resp = self.client.get('/test_tuple_3.py')

        assert resp.status_code == 202
        assert resp.headers['Content-Type'] == 'text/html'
        assert resp.headers['X-Test'] == 'PASS'
        assert resp.content == b'PASS'

        # 3rd .py resource
        resp = self.client.get('/test_h2_headers.py')

        assert resp.status_code == 203
        assert resp.headers['test'] == 'passed'
        assert resp.content == b''


class TestWorkersHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.worker.js': b'',
                   'foo.any.js': b''}

    def test_any_worker_html(self):
        self.run_wrapper_test('foo.any.worker.html',
                              'text/html', serve.WorkersHandler)

    def test_worker_html(self):
        self.run_wrapper_test('foo.worker.html',
                              'text/html', serve.WorkersHandler)


class TestWindowHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.window.js': b''}

    def test_window_html(self):
        self.run_wrapper_test('foo.window.html',
                              'text/html', serve.WindowHandler)

class TestWindowModulesHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.any.js': b'// META: global=window-module\n'}

    def test_any_window_module_html(self):
        self.run_wrapper_test('foo.any.window-module.html',
                              'text/html', serve.WindowModulesHandler)

class TestAnyHtmlHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.any.js': b'',
                   'foo.any.js.headers': b'X-Foo: 1',
                   '__dir__.headers': b'X-Bar: 2'}

    def test_any_html(self):
        self.run_wrapper_test('foo.any.html',
                              'text/html',
                              serve.AnyHtmlHandler,
                              headers=[('X-Foo', '1'), ('X-Bar', '2')])


class TestSharedWorkersHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.any.js': b'// META: global=sharedworker\n'}

    def test_any_sharedworkers_html(self):
        self.run_wrapper_test('foo.any.sharedworker.html',
                              'text/html', serve.SharedWorkersHandler)


class TestServiceWorkersHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'foo.any.js': b'// META: global=serviceworker\n'}

    def test_serviceworker_html(self):
        self.run_wrapper_test('foo.any.serviceworker.html',
                              'text/html', serve.ServiceWorkersHandler)


class TestClassicWorkerHandler(TestWrapperHandlerUsingServer):
    dummy_files = {'bar.any.js': b''}

    def test_any_work_js(self):
        self.run_wrapper_test('bar.any.worker.js', 'text/javascript',
                              serve.ClassicWorkerHandler)


if __name__ == '__main__':
    unittest.main()
