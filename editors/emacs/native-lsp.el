;;; native-lsp.el --- Emacs/Eglot host for RSS comparison  -*- lexical-binding: t; -*-
;;; emacs --batch -Q -l editors/emacs/native-lsp.el

(require 'cl-lib)
(require 'json)
(require 'project)
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

(defvar nlsp-server nil)

(defun nlsp-open-all ()
  (let ((project `(transient . ,nlsp-root))
        bufs)
    (dolist (path (sort (nlsp-mixed-files) #'string<))
      (let ((buf (find-file-noselect path)))
        (with-current-buffer buf
          (fundamental-mode))
        (push buf bufs)))
    (setq bufs (nreverse bufs))
    (with-current-buffer (car bufs)
      (eglot 'fundamental-mode project 'eglot-lsp-server (nlsp-cmd) "plaintext")
      (setq nlsp-server (or (eglot-current-server)
                            (ignore-errors (eglot--current-server-or-lose)))))
    bufs))

(defun nlsp-probe (buf)
  (with-current-buffer buf
    (goto-char (point-min))
    (let* ((uri (concat "file://" (expand-file-name (buffer-file-name))))
           (symbols (and nlsp-server
                         (ignore-errors
                           (jsonrpc-request
                            nlsp-server :textDocument/documentSymbol
                            `(:textDocument (:uri ,uri))
                            :timeout 3))))
           (objs (cond ((vectorp symbols) (append symbols nil))
                       ((listp symbols) symbols)
                       (t nil)))
           (count (length objs))
           (line 0)
           hover)
      (when objs
        (let* ((first (car objs))
               (range (plist-get (plist-get first :location) :range))
               (start (plist-get range :start)))
          (setq line (or (plist-get start :line) 0))))
      (when nlsp-server
        (let ((res (ignore-errors
                     (jsonrpc-request
                      nlsp-server :textDocument/hover
                      `(:textDocument (:uri ,uri)
                        :position (:line ,line :character 0))
                      :timeout 3))))
          (setq hover (or (plist-get (plist-get res :contents) :value) ""))))
      (list
       (cons 'name (file-name-nondirectory (buffer-file-name)))
       (cons 'language_id "fundamental")
       (cons 'symbol_count count)
       (cons 'hover (car (split-string (or hover "") "\n")))))))

(let* ((bufs (nlsp-open-all))
       (probes (mapcar #'nlsp-probe bufs)))
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
