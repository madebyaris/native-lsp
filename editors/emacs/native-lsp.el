;;; native-lsp.el --- Emacs/Eglot host for RSS comparison  -*- lexical-binding: t; -*-
;;; emacs --batch -Q -l editors/emacs/native-lsp.el

(require 'cl-lib)
(require 'json)
(require 'eglot)

(setq eglot-sync-connect 5)
(setq eglot-connect-timeout 8)
(setq eglot-autoshutdown nil)
(setq eglot-confirm-server-initiated-edits nil)
(setq noninteractive t)

(defconst nlsp-root (or (getenv "NATIVE_LSP_ROOT") default-directory))
(defconst nlsp-kind (or (getenv "NATIVE_LSP_KIND") "native"))
(defconst nlsp-report (or (getenv "NATIVE_LSP_REPORT")
                          (expand-file-name ".emacs-lsp-report.json" nlsp-root)))
(defconst nlsp-done (getenv "NATIVE_LSP_DONE"))

(defun nlsp-cmd ()
  (if (string= nlsp-kind "node")
      (list (or (getenv "NODE_BIN") "node")
            (expand-file-name "compare/node-lsp.mjs" nlsp-root))
    (list (or (getenv "NATIVE_LSP_BIN")
              (expand-file-name "target/release/native-lsp" nlsp-root)))))

(add-to-list 'eglot-server-programs `(fundamental-mode . ,(nlsp-cmd)))

(defun nlsp-mixed-files ()
  (directory-files (expand-file-name "testdata/mixed" nlsp-root) t
                   directory-files-no-dot-files-regexp))

(defun nlsp-open-all ()
  (let (bufs)
    (dolist (path (sort (nlsp-mixed-files) #'string<))
      (let ((buf (find-file-noselect path)))
        (with-current-buffer buf
          (fundamental-mode)
          (eglot-ensure)
          (push buf bufs))))
    (nreverse bufs)))

(defun nlsp-probe (buf)
  (with-current-buffer buf
    (let* ((server (eglot-current-server))
           (uri (eglot-path-to-uri (buffer-file-name)))
           (symbols
            (and server
                 (ignore-errors
                   (jsonrpc-request server :textDocument/documentSymbol
                                    (list :textDocument (list :uri uri))
                                    :timeout 3))))
           (count (if (vectorp symbols) (length symbols) 0))
           (line 0)
           hover)
      (when (and (vectorp symbols) (> count 0))
        (let ((first (aref symbols 0)))
          (setq line (or (plist-get (plist-get (plist-get first :location) :range) :start) 0))
          (when (listp line)
            (setq line (or (plist-get line :line) 0)))))
      (when server
        (let ((res (ignore-errors
                     (jsonrpc-request server :textDocument/hover
                                      (list :textDocument (list :uri uri)
                                            :position (list :line line :character 0))
                                      :timeout 3))))
          (setq hover (or (plist-get (plist-get res :contents) :value) ""))))
      (list
       (cons 'name (file-name-nondirectory (buffer-file-name)))
       (cons 'language_id "fundamental")
       (cons 'symbol_count count)
       (cons 'hover (car (split-string (or hover "") "\n")))))))

    (sleep-for 1)
    (let* ((bufs (nlsp-open-all))
       (probes (mapcar #'nlsp-probe bufs))
       (report `((host . "emacs")
                 (server . ,nlsp-kind)
                 (host_pid . ,(emacs-pid))
                 (probes . ,(vconcat (mapcar (lambda (p) (cons 'object p)) probes))))))
  ;; json.el wants alists; write a small hand-rolled object instead.
  (with-temp-file nlsp-report
    (insert (json-encode
             `(("host" . "emacs")
               ("server" . ,nlsp-kind)
               ("host_pid" . ,(emacs-pid))
               ("probes" . ,(cl-map 'vector
                                    (lambda (p)
                                      `(("name" . ,(cdr (assoc 'name p)))
                                        ("language_id" . ,(cdr (assoc 'language_id p)))
                                        ("symbol_count" . ,(cdr (assoc 'symbol_count p)))
                                        ("hover" . ,(or (cdr (assoc 'hover p)) ""))))
                                    probes))))))
  (when (and nlsp-done (not (string-empty-p nlsp-done)))
    (while (not (file-exists-p nlsp-done))
      (sleep-for 0.05))))

(kill-emacs 0)
