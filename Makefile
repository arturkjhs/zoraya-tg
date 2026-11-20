# zoraya-tg: TDLib on Zoraya, and python3 with the built-in module _tdjson
# (docs/tasks/TG-02-tdlib.md, docs/tdlib-report.md).
#
#   make fetch    download the pinned sources of ports/sources.lock into
#                 build/src (the only step with network)
#   make td       TDLib, cross-built with Zoraya's zcc/zc++: build/td-zoraya,
#                 its libraries collected in out/lib
#   make python   python3 with _tdjson: Zoraya's own python recipe into
#                 build/python-*, then relinked with Modules/Setup.local
#   make root     out/root: the volume's part (bin/python3, its library,
#                 the smoke test's programs in /home)
#   make smoke    the smoke test inside Zoraya under HVF, the image through
#                 Zoraya's EXTRA_ROOT; the transcript in build/logs/smoke.log
#
# tgd (docs/tasks/TG-03-tgd.md, docs/tgd-report.md):
#   make wheels   the pure-Python packages of ports/wheels.lock (network)
#   make tgd      out/tgd: bin/tgd (python3 with tgd's main) and usr/lib/tgd (tgd, tgclient's Qt-free
#                 packages from TGCLIENT=<dir>, the wheels, the Zoraya shim)
#   make host     tgd on the host: libtdjson from the same TDLib (build/host/td-lib),
#                 a venv with cryptography; run out/host/bin/tgd
#   make test-tgd-host   the protocol test on the host (no account)
#   make test-tgd-vm     tgd/vm (tgclient's view-models, protocol 3) on the host against a
#                        fake TDLib: no TDLib, no account, no build; python3.12 or newer
#   make test-tgd-window tgd's window front end over a Unix socket file, on the host (the same)
#   make smoke-tgd       tgd inside Zoraya under HVF (no account), build/logs/smoke-tgd.log
#   make smoke-tgd-net   the same with the Internet and the report's line (no account)
#   make run-tgd         Zoraya with the network, interactive, a kept disk (the hand check)
#
# The window (docs/tasks/TG-04-window.md, docs/tg-window.md, docs/tg-window-report.md):
#   make test-tg-host    the host tests of tg/window (the model, the sample backend, the widgets)
#   make tg              the native program, built here against ZTGW's crates by path
#   make overlay         overlay/*.patch applied to ZTGW (once; ZTGW must have no remote)
#   make tg-image        ZTGW's kernel with tg in its ramdisk and in the dock, and the volume
#   make test-tg-window  the window under HVF through Zoraya's GUI test runner, at scale 1 and 2
#   make run-tg          Zoraya's desktop with tg in the dock, interactive
#   make test-tg-socket  stage B: tg and tgd installed with saved grants, the window over
#                        tgd's socket (an invented account), at scale 1 and 2
#   make run-tg-login    stage B: the desktop with the network and a kept volume, for the
#                        real login by hand
#
# ZORAYA is the Zoraya checkout used as toolchain and image, read-only but for
# its build outputs; it must have run `make fetch-ports` and
# `make -f ports/Makefile cxx zlib openssl python`.
ZORAYA ?= $(abspath ../zoraya-tgw)
TG := $(abspath .)
B := $(TG)/build
OUT := $(TG)/out
LOGS := $(B)/logs
JOBS ?= 2
LOCK := ports/sources.lock
TGD_STAMP := $(OUT)/tgd/.stamp
TGD_LIB := $(OUT)/tgd/usr/lib/tgd

lock_field = $(shell awk '$$1 == "$(1)" { print $$$(2) }' $(LOCK))
TD_FILE := $(call lock_field,td,4)
TD_SHA := $(call lock_field,td,3)
TD_URL := $(call lock_field,td,5)
TD_SRC := $(B)/work/td
TD_HOST := $(B)/host/td-gen
TD_BUILD := $(B)/td-zoraya
ZTOOLS := $(ZORAYA)/build/ports/bin
ZCC := $(ZORAYA)/tools/zcc
ZCXX := $(ZORAYA)/tools/zc++
ZAR := $(ZTOOLS)/aarch64-unknown-zoraya-ar
# Host: cmake (Homebrew's), gperf (macOS's), the host's clang, OpenSSL for the
# host build of the generators (Homebrew's openssl@3).
CMAKE ?= cmake
HOST_OPENSSL ?= $(shell brew --prefix openssl@3 2>/dev/null)

logged = $(1) > $(LOGS)/$(2).log 2>&1 || { echo "$(2) failed: $(LOGS)/$(2).log"; tail -40 $(LOGS)/$(2).log; exit 1; }

.PHONY: fetch td python root smoke clean wheels tgd host test-tgd-host test-tgd-vm test-tgd-window smoke-tgd smoke-tgd-net run-tgd \
	overlay test-tg-host tg tg-image test-tg-window run-tg test-tg-socket run-tg-login tg-install test-tg-real-start walk-tg-login tg-upgrade
.SUFFIXES:

fetch: $(B)/src/$(TD_FILE)
$(B)/src/$(TD_FILE): $(LOCK)
	@mkdir -p $(B)/src
	@if [ ! -f $@ ]; then echo "fetch: td from $(TD_URL)"; curl -sSfL -o $@.part $(TD_URL) && mv $@.part $@; fi
	@echo "$(TD_SHA)  $@" | shasum -a 256 -c -s || { echo "fetch: $@ is not the locked SHA-256"; exit 1; }
	@touch $@

# ---------------------------------------------------------------- TDLib

# The tree: unpacked, patched (patches/td in name order), and its generated
# sources (the TL schemas' C++, the MIME tables by gperf) made by a host build
# of TDLib's generators: a cross build cannot run them (prepare_cross_compiling).
TD_SRC_STAMP := $(B)/obj/td-src.stamp
$(TD_SRC_STAMP): $(B)/src/$(TD_FILE) $(wildcard patches/td/*.patch)
	rm -rf $(B)/work/td-* $(TD_SRC) $(TD_HOST) && mkdir -p $(B)/work $(LOGS) $(dir $@)
	tar xzf $< -C $(B)/work && mv $(B)/work/td-[0-9a-f]* $(TD_SRC)
	for p in patches/td/*.patch; do echo "patch $$p"; patch -p1 -s -d $(TD_SRC) < $$p || exit 1; done
	mkdir -p $(TD_HOST)
	@$(call logged,$(CMAKE) -S $(TD_SRC) -B $(TD_HOST) -DCMAKE_BUILD_TYPE=Release -DOPENSSL_ROOT_DIR=$(HOST_OPENSSL),td-gen-configure)
	@$(call logged,$(CMAKE) --build $(TD_HOST) --target prepare_cross_compiling -j$(JOBS),td-gen)
	@touch $@

# The cross build: Zoraya's toolchain file (through ports/zoraya-tg.cmake,
# which adds Zoraya's OpenSSL and zlib), static, MinSizeRel; only the JSON
# client's libraries (tdjson_static and what it needs).
TD_STAMP := $(B)/obj/td.stamp
TD_LIBS := tdjson_static tdjson_private tdclient tdcore tdapi tdmtproto tde2e tddb tdsqlite tdnet tdactor tdutils
td: $(TD_STAMP)
$(TD_STAMP): $(TD_SRC_STAMP) ports/zoraya-tg.cmake
	rm -rf $(TD_BUILD) && mkdir -p $(TD_BUILD) $(OUT)/lib $(OUT)/include/td/telegram
	@$(call logged,ZORAYA=$(ZORAYA) $(CMAKE) -S $(TD_SRC) -B $(TD_BUILD) -DCMAKE_TOOLCHAIN_FILE=$(TG)/ports/zoraya-tg.cmake \
	  -DCMAKE_BUILD_TYPE=MinSizeRel -DTD_ENABLE_JNI=OFF -DTD_ENABLE_DOTNET=OFF -DOPENSSL_USE_STATIC_LIBS=ON -DCMAKE_EXPORT_COMPILE_COMMANDS=ON,td-configure)
	@date +%s > $(LOGS)/td-build.start
	@$(call logged,ZORAYA=$(ZORAYA) $(CMAKE) --build $(TD_BUILD) --target tdjson_static -j$(JOBS),td-build)
	@echo "td: built in $$(( $$(date +%s) - $$(cat $(LOGS)/td-build.start) )) s at -j$(JOBS)"
	@# INV-0109-2 of Zoraya, for this port: every compile ran zcc or zc++, none named Linux.
	@grep -q -e 'Building CXX' $(LOGS)/td-build.log
	@grep -q -e "$(ZORAYA)/tools/zc++" $(TD_BUILD)/compile_commands.json && grep -q -e "$(ZORAYA)/tools/zcc" $(TD_BUILD)/compile_commands.json
	@if grep -n -e "-D__linux" $(TD_BUILD)/compile_commands.json; then exit 1; fi
	for l in $(TD_LIBS); do f=$$(find $(TD_BUILD) -name lib$$l.a | head -1); [ -n "$$f" ] || { echo "td: no lib$$l.a"; exit 1; }; cp $$f $(OUT)/lib/; done
	cp $(TD_SRC)/td/telegram/td_json_client.h $(TD_SRC)/td/telegram/td_log.h $(OUT)/include/td/telegram/
	cp $(TD_BUILD)/td/telegram/tdjson_export.h $(OUT)/include/td/telegram/
	@# The archives stay separate: merging them by extraction loses members (two
	@# libraries have a misc.cpp.o, libtde2e.a has two e2e_api.cpp.o), and lld,
	@# unlike GNU ld, resolves across archives in any order.
	@echo "td: the $(words $(TD_LIBS)) archives $$(cat $(addprefix $(OUT)/lib/lib,$(addsuffix .a,$(TD_LIBS))) | wc -c | awk '{ printf "%d", $$1 / 1024 }') KiB"
	@touch $@

# The names TDLib needs that libc.a does not export (ports/tdshim.c).
$(OUT)/lib/libtdshim.a: ports/tdshim.c
	@mkdir -p $(B)/obj $(OUT)/lib
	$(ZCC) -O2 -Wall -Wextra -c -o $(B)/obj/tdshim.o ports/tdshim.c
	rm -f $@ && $(ZAR) rcs $@ $(B)/obj/tdshim.o

# ---------------------------------------------------------------- python3

# Zoraya's python recipe, unchanged, into directories of this repository: the
# same pinned source and patches, configure answers and flags. Then
# Modules/Setup.local names _tdjson (static, a built-in module) and python3 is
# linked again, by zc++: TDLib is C++ (libc++, libc++abi, libunwind and the
# .eh_frame_hdr). Until Zoraya's python build takes extra built-in modules from
# outside (docs/requests.md, R1), this is the way in.
PY_BUILD := $(B)/python-build
PY_STAGE := $(B)/python-stage
PY_ROOT := $(B)/rootfs-python
PY_STAMP := $(B)/obj/python-tdjson.stamp
python: $(PY_STAMP)
$(PY_ROOT)/.stamp:
	@mkdir -p $(LOGS)
	$(MAKE) -C $(ZORAYA) -f ports/Makefile PYTHON_BUILD=$(PY_BUILD) PYTHON_STAGE=$(PY_STAGE) PYTHON_ROOT=$(PY_ROOT) LOGS=$(LOGS) $(PY_ROOT)/.stamp
$(PY_STAMP): $(PY_ROOT)/.stamp $(TD_STAMP) $(OUT)/lib/libtdshim.a modules/_tdjson.c modules/_aead.c
	@# _aead: AES-GCM over the libcrypto python3 already links (_ssl, _hashlib), for vault.py.
	printf '%s\n' '# zoraya-tg: TDLib JSON client and AES-GCM, built in (Makefile, make python)' '*static*' \
	  '_tdjson $(TG)/modules/_tdjson.c -I$(OUT)/include -DTDJSON_STATIC_DEFINE $(addprefix $(OUT)/lib/lib,$(addsuffix .a,$(TD_LIBS) tdshim))' \
	  '_aead $(TG)/modules/_aead.c -I$(ZORAYA)/build/ports/deps/include' \
	  > $(PY_BUILD)/Modules/Setup.local
	@# makesetup keeps an absolute source as it is and puts its object under Modules/ + that path.
	mkdir -p $(PY_BUILD)/Modules$(TG)/modules
	@# makesetup turns Setup.local into the Makefile's lines first (CPython's make
	@# stops after that, "re-run make"); the program is python.exe on a macOS host,
	@# whose file system does not tell it from the Python/ directory (BUILDEXE).
	@$(call logged,PATH=$(ZTOOLS):$$PATH $(MAKE) -C $(PY_BUILD) Makefile,python-tdjson-setup)
	@$(call logged,PATH=$(ZTOOLS):$$PATH $(MAKE) -C $(PY_BUILD) LINKCC=$(ZCXX) -j3 $$(sed -n 's/^BUILDPYTHON=[[:space:]]*//p' $(PY_BUILD)/Makefile | sed 's/$$(BUILDEXE)/'"$$(sed -n 's/^BUILDEXE=[[:space:]]*//p' $(PY_BUILD)/Makefile)"'/'),python-tdjson)
	cp $(PY_BUILD)/python$$(sed -n 's/^BUILDEXE=[[:space:]]*//p' $(PY_BUILD)/Makefile) $(B)/python3-tdjson
	$(ZORAYA)/target/debug/zoraya-elf-check $(B)/python3-tdjson
	@grep -q '_tdjson' $(PY_BUILD)/Modules/config.c || { echo "python: _tdjson is not in Modules/config.c"; exit 1; }
	@grep -q '_aead' $(PY_BUILD)/Modules/config.c || { echo "python: _aead is not in Modules/config.c"; exit 1; }
	@touch $@

# ---------------------------------------------------------------- the image part

root: $(OUT)/root/.stamp
$(OUT)/root/.stamp: $(PY_STAMP) $(TGD_STAMP) $(wildcard tests/*.py)
	rm -rf $(OUT)/root && mkdir -p $(OUT)/root/bin $(OUT)/root/usr/lib $(OUT)/root/home
	cp $(B)/python3-tdjson $(OUT)/root/bin/python3
	cp -R $(PY_ROOT)/usr/lib/. $(OUT)/root/usr/lib/
	cp -R $(OUT)/tgd/. $(OUT)/root/
	rm -f $(OUT)/root/.stamp
	cp tests/*.py $(OUT)/root/home/
	@echo "root: python3 $$(du -k $(OUT)/root/bin/python3 | cut -f1) KiB; the part $$(du -sk $(OUT)/root | cut -f1) KiB"
	@touch $@

# ---------------------------------------------------------------- smoke test

# The volume: Zoraya's rootfs and fonts with out/root over them, made by
# Zoraya's tools/run-disk.sh from EXTRA_ROOT. One guest, under HVF, through
# Zoraya's test runner and run-qemu.sh (Zoraya's CLAUDE.md: QEMU only so).
SMOKE_DISK := $(B)/smoke/disk.img
SMOKE_TIMEOUT ?= 300
smoke: $(OUT)/root/.stamp tests/smoke.script
	@mkdir -p $(B)/smoke $(LOGS)
	cd $(ZORAYA) && $(MAKE) build tools-storage && cargo build -q -p zoraya-test-runner
	rm -f $(SMOKE_DISK) $(SMOKE_DISK).key
	cd $(ZORAYA) && EXTRA_ROOT=$(OUT)/root DISK_SIZE=256M tools/run-disk.sh $(SMOKE_DISK)
	cd $(ZORAYA) && target/debug/zoraya-test-runner --timeout $(SMOKE_TIMEOUT) --script $(TG)/tests/smoke.script --label tg-smoke-hvf -- \
	  tools/run-qemu.sh --test --keys --mem 512M --disk $(SMOKE_DISK) --accel hvf build/zoraya-kernel.elf; rc=$$?; \
	  cp target/test-logs/tg-smoke-hvf.log $(LOGS)/smoke.log; exit $$rc

# ---------------------------------------------------------------- tgd

# TGCLIENT is a checkout of tgclient (code only); its Qt-free packages are packed into
# out/tgd, never into this repository. Left out: models/, ui/, app.py, selftest.py,
# devbundle.py (Qt), and the services that need numpy or self-update (embeddings, search,
# search_index, updates).
TGCLIENT ?= $(HOME)/workspace/tgclient
TGC_SRC := $(TGCLIENT)/src/tgclient
TGC_TOP := __init__.py accounts.py cli.py config.py prefs.py vault.py
TGC_DIRS := td store services
TGC_LEAVE := services/embeddings.py services/search.py services/search_index.py services/updates.py
WHEELS := $(B)/src/wheels
WHEEL_LOCK := ports/wheels.lock

wheels: $(WHEELS)/.stamp
$(WHEELS)/.stamp: $(WHEEL_LOCK)
	@mkdir -p $(WHEELS)
	@grep -v '^#' $(WHEEL_LOCK) | while read name version sha url; do \
	  f=$(WHEELS)/$$(basename $$url); \
	  [ -f $$f ] || { echo "wheels: $$name $$version"; curl -sSfL -o $$f.part $$url && mv $$f.part $$f; } || exit 1; \
	  echo "$$sha  $$f" | shasum -a 256 -c -s || { echo "wheels: $$f is not the locked SHA-256"; exit 1; }; \
	done
	@touch $@

tgd: $(TGD_STAMP)
# /bin/tgd: python3's objects with tgd's main (tgd/tgd_main.c, ports/tgd-link.mk).
$(B)/tgd.elf: $(PY_STAMP) tgd/tgd_main.c ports/tgd-link.mk
	@$(call logged,PATH=$(ZTOOLS):$$PATH $(MAKE) -C $(PY_BUILD) -f Makefile -f $(TG)/ports/tgd-link.mk LINKCC=$(ZCXX) TGD_MAIN=$(TG)/tgd/tgd_main.c tgd-elf,tgd-elf)
	cp $(PY_BUILD)/tgd.elf $@
	$(ZORAYA)/target/debug/zoraya-elf-check $@
$(TGD_STAMP): $(B)/tgd.elf $(WHEELS)/.stamp $(shell find tgd -type f -not -name '*.pyc' -not -name tgd_main.c) $(shell find $(TGC_SRC) -name '*.py' 2>/dev/null)
	@[ -f $(TGC_SRC)/td/client.py ] || { echo "tgd: no tgclient checkout at TGCLIENT=$(TGCLIENT)"; exit 1; }
	rm -rf $(OUT)/tgd && mkdir -p $(OUT)/tgd/bin $(TGD_LIB)/tgclient $(TGD_LIB)/site
	cp $(B)/tgd.elf $(OUT)/tgd/bin/tgd
	cp -R tgd/tgd tgd/vm tgd/zoraya tgd/tgd.py $(TGD_LIB)/
	cd $(TGC_SRC) && cp $(TGC_TOP) $(TGD_LIB)/tgclient/ && \
	  find $(TGC_DIRS) -type f \( -name '*.py' -o -name '*.json' \) | while read f; do \
	    mkdir -p $(TGD_LIB)/tgclient/$$(dirname $$f) && cp $$f $(TGD_LIB)/tgclient/$$f; done
	cd $(TGD_LIB)/tgclient && rm -f $(TGC_LEAVE)
	@# The commit of tgclient packed, without anything of its working tree's state.
	git -C $(TGCLIENT) rev-parse HEAD > $(TGD_LIB)/tgclient/COMMIT
	@grep -v '^#' $(WHEEL_LOCK) | while read name version sha url; do \
	  python3 -I -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' $(WHEELS)/$$(basename $$url) $(TGD_LIB)/site || exit 1; done
	find $(OUT)/tgd -name '__pycache__' -prune -exec rm -rf {} +
	@if grep -rln -e 'PySide6' -e 'import numpy' $(TGD_LIB)/tgclient; then echo "tgd: Qt or numpy in the packed tgclient"; exit 1; fi
	@echo "tgd: /bin/tgd $$(du -k $(OUT)/tgd/bin/tgd | cut -f1) KiB; usr/lib/tgd $$(find $(TGD_LIB) -name '*.py' | wc -l | tr -d ' ') Python files, $$(du -sk $(TGD_LIB) | cut -f1) KiB; tgclient $$(cut -c1-8 $(TGD_LIB)/tgclient/COMMIT)"
	@touch $@

# The host: libtdjson from the same patched TDLib tree (the Zoraya patches change nothing
# outside TD_ZORAYA/__zoraya__), built by the host's clang; Homebrew's python with a venv
# that has only cryptography (tgclient's pin; the pure-Python packages come from out/tgd).
HOST_PY ?= /opt/homebrew/bin/python3.14
HOST_TD := $(B)/host/td-lib
HOST_OUT := $(OUT)/host
HOST_VENV := $(B)/host/venv
$(HOST_OUT)/lib/.stamp: $(TD_SRC_STAMP)
	rm -rf $(HOST_TD) && mkdir -p $(HOST_TD) $(HOST_OUT)/lib $(LOGS)
	@$(call logged,$(CMAKE) -S $(TD_SRC) -B $(HOST_TD) -DCMAKE_BUILD_TYPE=Release -DOPENSSL_ROOT_DIR=$(HOST_OPENSSL),host-td-configure)
	@date +%s > $(LOGS)/host-td.start
	@$(call logged,$(CMAKE) --build $(HOST_TD) --target tdjson -j$(JOBS),host-td-build)
	@echo "host td: built in $$(( $$(date +%s) - $$(cat $(LOGS)/host-td.start) )) s at -j$(JOBS)"
	cp $$(find $(HOST_TD) -maxdepth 1 -name 'libtdjson.*.dylib' -type f | head -1) $(HOST_OUT)/lib/libtdjson.dylib
	@touch $@
$(HOST_VENV)/.stamp: ports/host-requirements.txt
	rm -rf $(HOST_VENV) && $(HOST_PY) -m venv $(HOST_VENV)
	$(HOST_VENV)/bin/pip install -q --require-hashes --only-binary :all: -r ports/host-requirements.txt
	@touch $@
host: $(HOST_OUT)/lib/.stamp $(HOST_VENV)/.stamp $(TGD_STAMP)
	rm -rf $(HOST_OUT)/bin $(HOST_OUT)/usr && mkdir -p $(HOST_OUT)/bin $(HOST_OUT)/usr/lib
	ln -s $(TGD_LIB) $(HOST_OUT)/usr/lib/tgd
	@# The launcher with the venv's python, and TDJSON_PATH to the host library.
	{ echo '#!/bin/sh'; echo 'TDJSON_PATH="$${TDJSON_PATH:-$(HOST_OUT)/lib/libtdjson.dylib}" exec $(HOST_VENV)/bin/python -I $(HOST_OUT)/usr/lib/tgd/tgd.py "$$@"'; } > $(HOST_OUT)/bin/tgd
	chmod 755 $(HOST_OUT)/bin/tgd
	@echo "host: run $(HOST_OUT)/bin/tgd --test-dc"

# tgd/vm needs only tgclient's Qt-free packages (TGCLIENT) and a Python >= 3.12.
VM_PY ?= python3.12
test-tgd-vm:
	$(VM_PY) -I tests/tgd_vm.py $(TGC_SRC)/..

# tgd's window front end (tgd/tgd/window.py): protocol 3 over a Unix socket file, on the host.
test-tgd-window:
	$(VM_PY) -I tests/tgd_window.py $(TGC_SRC)/..

test-tgd-host: host
	$(HOST_VENV)/bin/python -I tests/tgd_protocol.py $(HOST_OUT)/bin/tgd

# tgd inside Zoraya (no account): tests/smoke-tgd.script, on the image of make root.
SMOKE_TGD_DISK := $(B)/smoke/tgd-disk.img
smoke-tgd: $(OUT)/root/.stamp tests/smoke-tgd.script
	@mkdir -p $(B)/smoke $(LOGS)
	cd $(ZORAYA) && $(MAKE) build tools-storage && cargo build -q -p zoraya-test-runner
	rm -f $(SMOKE_TGD_DISK) $(SMOKE_TGD_DISK).key
	cd $(ZORAYA) && EXTRA_ROOT=$(OUT)/root DISK_SIZE=384M tools/run-disk.sh $(SMOKE_TGD_DISK)
	cd $(ZORAYA) && target/debug/zoraya-test-runner --timeout $(SMOKE_TIMEOUT) --script $(TG)/tests/smoke-tgd.script --label tg-smoke-tgd -- \
	  tools/run-qemu.sh --test --keys --mem 512M --disk $(SMOKE_TGD_DISK) --accel hvf build/zoraya-kernel.elf; rc=$$?; \
	  cp target/test-logs/tg-smoke-tgd.log $(LOGS)/smoke-tgd.log; exit $$rc

# The network leg (needs the Internet): the exact line of the report (tgd --grant --test-dc,
# made by the host tgd from the same CIDR list) put into the script; QEMU's network open.
SMOKE_TGD_NET_DISK := $(B)/smoke/tgd-net-disk.img
TGD_MEM ?= 1G
smoke-tgd-net: $(OUT)/root/.stamp tests/smoke-tgd-net.script tests/expand_script.py host
	@mkdir -p $(B)/smoke $(LOGS)
	python3 tests/expand_script.py tests/smoke-tgd-net.script "$$($(HOST_OUT)/bin/tgd --grant --test-dc)" > $(B)/smoke/smoke-tgd-net.script
	cd $(ZORAYA) && $(MAKE) build tools-storage && cargo build -q -p zoraya-test-runner
	rm -f $(SMOKE_TGD_NET_DISK) $(SMOKE_TGD_NET_DISK).key
	cd $(ZORAYA) && EXTRA_ROOT=$(OUT)/root DISK_SIZE=384M tools/run-disk.sh $(SMOKE_TGD_NET_DISK)
	cd $(ZORAYA) && target/debug/zoraya-test-runner --timeout $(SMOKE_TIMEOUT) --script $(B)/smoke/smoke-tgd-net.script --label tg-smoke-tgd-net -- \
	  tools/run-qemu.sh --test --keys --net --net-open --mem $(TGD_MEM) --disk $(SMOKE_TGD_NET_DISK) --accel hvf build/zoraya-kernel.elf; rc=$$?; \
	  cp target/test-logs/tg-smoke-tgd-net.log $(LOGS)/smoke-tgd-net.log; exit $$rc

# For the architect's hand check (docs/tgd-report.md): Zoraya with the network, interactive, on a
# disk that is kept between runs (build/run/tgd-disk.img: ~/tg and its database stay). FRESH=1
# makes the disk again from out/root (after make tgd root), which loses ~/tg.
RUN_TGD_DISK := $(B)/run/tgd-disk.img
run-tgd: $(OUT)/root/.stamp
	@mkdir -p $(B)/run
	cd $(ZORAYA) && $(MAKE) build tools-storage
	@if [ -n "$(FRESH)" ] || [ ! -f $(RUN_TGD_DISK) ]; then rm -f $(RUN_TGD_DISK) $(RUN_TGD_DISK).key; \
	  cd $(ZORAYA) && EXTRA_ROOT=$(OUT)/root DISK_SIZE=384M tools/run-disk.sh $(RUN_TGD_DISK); fi
	@echo "run-tgd: the line is in docs/tgd-report.md (or: tgd --grant); power off with poweroff"
	cd $(ZORAYA) && tools/run-qemu.sh --net --mem $(TGD_MEM) --disk $(RUN_TGD_DISK) --accel hvf build/zoraya-kernel.elf

# ---------------------------------------------------------------- the window (TG-04)

# ZTGW: a local clone of Zoraya with no remote, beside this repository, at the commit in
# overlay/BASE, with overlay/*.patch applied (nothing of this repository can be pushed from
# it). The crates of tg/ name it by path (../zoraya-tgw), so it is not a variable there.
ZTGW := $(abspath ../zoraya-tgw)
TG_ELF := $(TG)/target/aarch64-unknown-none/user/tg
TGW_DISK := $(B)/tgw/disk.img
TGW_SCALES ?= 1 2
TGW_TIMEOUT ?= 420
# What the volume carries besides Zoraya's rootfs: tgd's part when it is built (stage B's
# image), nothing otherwise.
TGW_ROOT := $(if $(wildcard $(OUT)/root/.stamp),$(OUT)/root:,)
TGW_DISK_SIZE := $(if $(TGW_ROOT),384M,64M)
# ... and the settings of the test (the keyboard's layouts: us, ua, ru).
TGW_EXTRA_ROOT := $(TGW_ROOT)$(TG)/tests/tg-window.root
# A fresh volume for a run, through Zoraya's run-disk.sh.
tgw_disk = rm -f $(TGW_DISK) $(TGW_DISK).key && cd $(ZTGW) && EXTRA_ROOT=$(TGW_EXTRA_ROOT) DISK_SIZE=$(TGW_DISK_SIZE) tools/run-disk.sh $(TGW_DISK)

overlay:
	@[ -d $(ZTGW)/.git ] || { echo "overlay: no Zoraya clone at $(ZTGW) (docs/tasks/TG-04-window.md)"; exit 1; }
	@[ -z "$$(git -C $(ZTGW) remote)" ] || { echo "overlay: $(ZTGW) has a remote; it must have none"; exit 1; }
	@for p in overlay/*.patch; do \
	  if git -C $(ZTGW) apply --reverse --check $(TG)/$$p 2>/dev/null; then echo "overlay: $$p is applied"; \
	  else git -C $(ZTGW) apply $(TG)/$$p && echo "overlay: applied $$p" || exit 1; fi; done

test-tg-host:
	CARGO_BUILD_JOBS=$(JOBS) cargo test -p tg-window

tg:
	CARGO_BUILD_JOBS=$(JOBS) cargo build -p tg --target aarch64-unknown-none --profile user
	@echo "tg: $$(wc -c < $(TG_ELF) | awk '{ printf "%d", $$1 / 1024 }') KiB"

# Zoraya's kernel with tg in its ramdisk (EXTRA_PROGRAMS) and in the dock (EXTRA_APPS), its
# storage tools and its test runner; then the volume.
# The ramdisk's copy is `tg-sample` (the dock's tile, the sample backend: it has no start-up
# record and so no grant), so that the name `tg` stays free for the installed program
# (install refuses a name an image command has).
TG_SAMPLE := $(B)/tgw/tg-sample
tg-image: tg overlay
	@mkdir -p $(B)/tgw && cp $(TG_ELF) $(TG_SAMPLE)
	cd $(ZTGW) && CARGO_BUILD_JOBS=$(JOBS) $(MAKE) build tools-storage EXTRA_PROGRAMS=$(TG_SAMPLE) EXTRA_APPS=tg-sample && \
	  CARGO_BUILD_JOBS=$(JOBS) cargo build -q -p zoraya-test-runner
	@mkdir -p $(B)/tgw $(LOGS)
	$(tgw_disk)

# The script plays at each scale on a fresh volume; its screens (build/shots of ZTGW, PPM)
# become docs/shots/*.png. One guest at a time.
test-tg-window: tg-image tests/tg-window.script tests/ppm2png.py
	@for scale in $(TGW_SCALES); do \
	  label=tg-window-$$scale; \
	  ( $(tgw_disk) && \
	    target/debug/zoraya-test-runner --timeout $(TGW_TIMEOUT) --gui-script $(TG)/tests/tg-window.script \
	      --qmp build/$$label.qmp --label $$label -- \
	      tools/run-qemu.sh --test --gui --gui-scale $$scale --mem 512M --qmp build/$$label.qmp \
	      --disk $(TGW_DISK) --accel hvf build/zoraya-kernel.elf ); rc=$$?; \
	  cp $(ZTGW)/target/test-logs/$$label.log $(LOGS)/$$label.log 2>/dev/null; \
	  [ $$rc = 0 ] || exit $$rc; \
	  python3 tests/ppm2png.py $(ZTGW)/build/shots docs/shots $$label || exit 1; \
	done

# Measurements (docs/tg-window-report.md), not tests: make measure-tg-memory (tg alone in
# a room, `top` in the first terminal) and make measure-tg-layout SCALE=1|2 (a chat of 1000
# messages: the first frame of a new tree, the paint times). Their screens stay in ZTGW's
# build/shots; the log is build/logs/tg-NAME.log.
measure-tg-%: tg-image tests/tg-%.script
	( $(tgw_disk) && target/debug/zoraya-test-runner --timeout 400 --gui-script $(TG)/tests/tg-$*.script \
	    --qmp build/tg-$*.qmp --label tg-$* -- \
	    tools/run-qemu.sh --test --gui --gui-scale $(or $(SCALE),1) --mem 512M --qmp build/tg-$*.qmp \
	    --disk $(TGW_DISK) --accel hvf build/zoraya-kernel.elf ); rc=$$?; \
	  cp $(ZTGW)/target/test-logs/tg-$*.log $(LOGS)/tg-$*.log 2>/dev/null; exit $$rc

run-tg: tg-image
	cd $(ZTGW) && tools/run-qemu.sh --gui --gui-scale $(or $(SCALE),2) --mem 512M --disk $(TGW_DISK) --accel hvf build/zoraya-kernel.elf

# ---------------------------------------------------------------- stage B: installed, over the socket
# The volume's part for stage B: python3 and its library, tgd's library, and the packages
# /packages/tg and /packages/tgd, which `install` copies to /apps with their saved grants
# (docs/tg-window-report.md has the lines). Not /bin/tgd: /bin is on shell.path, which comes
# before the installed names (ADR-0396 item 6), so tgd would start without its grants.
STAGEB := $(OUT)/stageb
# The architect's kept volume (run-tg-login, tg-install: its api id, hash and login), and the
# tests' own (test-tg-socket, test-tg-real-start make it again each run): never the same file.
TGS_DISK := $(B)/tgs/disk.img
TGT_DISK := $(B)/tgs-test/disk.img
TGS_DISK_SIZE ?= 768M
TGS_SCALES ?= 1 2
TGS_TIMEOUT ?= 600
TGS_MEM ?= 1024M
# QEMU's cocoa display without zoom-to-fit: with it, the window tells the guest its own size,
# which on this Mac mini's 1920x1080 screen became 640x480 at once, and Zoraya's desktop
# followed (too small for tg's window, the dock and the terminals; R26).
TGS_DISPLAY ?= cocoa,left-command-key=on
$(STAGEB)/.stamp: $(OUT)/root/.stamp $(TG_ELF) packages/tg/tg.cmd packages/tgd/tgd.cmd
	rm -rf $(STAGEB) && mkdir -p $(STAGEB)/packages/tg $(STAGEB)/packages/tgd
	cp -R $(OUT)/root/. $(STAGEB)/ && rm -f $(STAGEB)/bin/tgd $(STAGEB)/.stamp
	cp $(TG_ELF) $(STAGEB)/packages/tg/tg && cp packages/tg/tg.cmd $(STAGEB)/packages/tg/
	cp $(B)/tgd.elf $(STAGEB)/packages/tgd/tgd && cp packages/tgd/tgd.cmd $(STAGEB)/packages/tgd/
	@echo "stageb: the packages tg $$(du -k $(STAGEB)/packages/tg/tg | cut -f1) KiB, tgd $$(du -k $(STAGEB)/packages/tgd/tgd | cut -f1) KiB"
	@touch $@
tgs_disk = rm -f $(1) $(1).key && mkdir -p $(dir $(1)) && cd $(ZTGW) && EXTRA_ROOT=$(STAGEB):$(TG)/tests/tg-window.root DISK_SIZE=$(TGS_DISK_SIZE) tools/run-disk.sh $(1)

# The two programs installed on the stage B volume, on the console (tests/tg-install.script,
# its long lines typed in pieces by tests/expand_script.py): install needs the volume's root,
# which only the console's shell holds; a terminal of the desktop holds what home gives.
TGS_INSTALL := $(B)/tgs/tg-install.script
$(TGS_INSTALL): tests/tg-install.script tests/expand_script.py tgd/tgd/net.py tgd/tgd/telegram-cidr.txt
	@mkdir -p $(B)/tgs
	python3 tests/expand_script.py tests/tg-install.script "$$(cd tgd && python3 -c 'from tgd import net; print(net.install_line())')" > $@
tgs_install = cd $(ZTGW) && target/debug/zoraya-test-runner --timeout 600 --script $(TGS_INSTALL) --label tg-install -- \
	  tools/run-qemu.sh --test --keys --mem $(TGS_MEM) --disk $(1) --accel hvf build/zoraya-kernel.elf; rc=$$?; \
	  cp target/test-logs/tg-install.log $(LOGS)/tg-install.log 2>/dev/null; [ $$rc = 0 ]

# The window over tgd's socket (tests/tg-socket.script), on a fresh volume for each scale:
# the console's boot installs both, the desktop's boot runs tgd --window --synthetic (an
# invented account: no TDLib, no network) and tg from the terminal; the login, the list, a
# chat, the jump to the first unread message, a send, a failed send and its retry, tgd gone
# and back. One guest at a time.
test-tg-socket: tg-image $(STAGEB)/.stamp $(TGS_INSTALL) tests/tg-socket.script tests/ppm2png.py
	@for scale in $(TGS_SCALES); do \
	  label=tg-socket-$$scale; \
	  ( $(call tgs_disk,$(TGT_DISK)) ) && ( $(call tgs_install,$(TGT_DISK)) ) || exit 1; \
	  ( cd $(ZTGW) && target/debug/zoraya-test-runner --timeout $(TGS_TIMEOUT) --gui-script $(TG)/tests/tg-socket.script \
	      --qmp build/$$label.qmp --label $$label -- \
	      tools/run-qemu.sh --test --gui --gui-scale $$scale --mem $(TGS_MEM) --qmp build/$$label.qmp \
	      --disk $(TGT_DISK) --accel hvf build/zoraya-kernel.elf ); rc=$$?; \
	  cp $(ZTGW)/target/test-logs/$$label.log $(LOGS)/$$label.log 2>/dev/null; \
	  [ $$rc = 0 ] || exit $$rc; \
	  python3 tests/ppm2png.py $(ZTGW)/build/shots docs/shots $$label || exit 1; \
	done

# The real tgd (TDLib) serving the window, with placeholder credentials and no network
# (tests/tg-real-start.script): the start the architect's login goes through. Scale 1.
test-tg-real-start: tg-image $(STAGEB)/.stamp $(TGS_INSTALL) tests/tg-real-start.script
	( $(call tgs_disk,$(TGT_DISK)) ) && ( $(call tgs_install,$(TGT_DISK)) ) || exit 1
	( cd $(ZTGW) && target/debug/zoraya-test-runner --timeout $(TGS_TIMEOUT) --gui-script $(TG)/tests/tg-real-start.script \
	    --qmp build/tg-real-1.qmp --label tg-real-1 -- \
	    tools/run-qemu.sh --test --gui --gui-scale 1 --mem $(TGS_MEM) --qmp build/tg-real-1.qmp \
	    --disk $(TGT_DISK) --accel hvf build/zoraya-kernel.elf ); rc=$$?; \
	  cp $(ZTGW)/target/test-logs/tg-real-1.log $(LOGS)/tg-real-1.log 2>/dev/null; \
	  [ $$rc = 0 ] && python3 tests/ppm2png.py $(ZTGW)/build/shots docs/shots tg-real-1

# For the architect: a fresh stage B volume with both installed (refused while one is kept,
# unless FRESH=1).
tg-install: tg-image $(STAGEB)/.stamp $(TGS_INSTALL)
	@if [ -f $(TGS_DISK) ] && [ -z "$(FRESH)" ]; then echo "tg-install: $(TGS_DISK) is kept (the login is on it); FRESH=1 makes it again"; exit 1; fi
	$(call tgs_disk,$(TGS_DISK))
	$(call tgs_install,$(TGS_DISK))

# The architect's run: Zoraya's desktop with the network and a kept volume (FRESH=1 makes
# it again), the packages on it, for the real login by hand (docs/tg-window-report.md).
run-tg-login: tg-image $(STAGEB)/.stamp $(TGS_INSTALL)
	@if [ -n "$(FRESH)" ] || [ ! -f $(TGS_DISK) ]; then ( $(call tgs_disk,$(TGS_DISK)) ) && ( $(call tgs_install,$(TGS_DISK)) ) || exit 1; fi
	cd $(ZTGW) && tools/run-qemu.sh --gui --gui-scale $(or $(SCALE),1) --display-opts $(TGS_DISPLAY) --net --mem $(TGS_MEM) --disk $(TGS_DISK) --accel hvf build/zoraya-kernel.elf

# New packages on a kept volume (tests/tg-upgrade.script): tg's two files and tgd's
# description fetched over /packages on a console boot from a host server of those three
# files (127.0.0.1 only, reached through one guestfwd rule; the guest's network is
# restrict=on), then install of each (tgd with its whole line), checked against each package's identity
# computed here. It runs on a copy of the volume, which replaces it only when every step
# passed; otherwise the copy is deleted and the volume is as it was. So at most one volume
# holds the login when it ends. tgd's files (the api id and hash, the session) are not
# touched. VOL is the volume (default: the kept one).
VOL ?= $(TGS_DISK)
TGU := $(B)/tgu
# A package's identity as install prints it (ADR-0396 item 4): sha256 of the program and the
# description, each after its length (8 bytes, little-endian); the first 8 hex digits.
TGU_ID = python3 -c 'import hashlib, struct, sys; p, d = (open(f, "rb").read() for f in sys.argv[1:]); \
	print(hashlib.sha256(struct.pack("<Q", len(p)) + p + struct.pack("<Q", len(d)) + d).hexdigest()[:8])'
tgu_run = rm -rf $(TGU) && mkdir -p $(TGU)/www && \
	  cp $(STAGEB)/packages/tg/tg $(STAGEB)/packages/tg/tg.cmd $(STAGEB)/packages/tgd/tgd.cmd $(TGU)/www/ && \
	  (cd $(ZTGW) && cargo build -q -p zoraya-test-httpd) && \
	  port=$$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])') && \
	  tgh=$$($(TGU_ID) $(STAGEB)/packages/tg/tg $(STAGEB)/packages/tg/tg.cmd) && \
	  tgdh=$$($(TGU_ID) $(STAGEB)/packages/tgd/tgd $(STAGEB)/packages/tgd/tgd.cmd) && \
	  sed -e "s/@PORT@/$$port/g" -e "s/@TGHASH@/$$tgh/g" -e "s/@TGDHASH@/$$tgdh/g" tests/tg-upgrade.script > $(TGU)/raw.script && \
	  python3 tests/expand_script.py $(TGU)/raw.script "$$(cd tgd && python3 -c 'from tgd import net; print(net.install_line())')" > $(TGU)/tg-upgrade.script && \
	  cp $(1) $(TGU)/disk.img && cp $(1).key $(TGU)/disk.img.key && \
	  { python3 -m http.server --bind 127.0.0.1 --directory $(TGU)/www $$port > $(TGU)/httpd.log 2>&1 & srv=$$!; \
	  sleep 1; ( cd $(ZTGW) && target/debug/zoraya-test-runner --timeout 600 --script $(TGU)/tg-upgrade.script --label tg-upgrade -- \
	    tools/run-qemu.sh --test --keys --net --net-guestfwd "10.0.2.50:$$port=$(ZTGW)/target/debug/zoraya-test-httpd --relay 127.0.0.1:$$port" \
	    --mem $(TGS_MEM) --disk $(TGU)/disk.img --accel hvf build/zoraya-kernel.elf ); rc=$$?; kill $$srv; \
	  cp $(ZTGW)/target/test-logs/tg-upgrade.log $(LOGS)/tg-upgrade.log 2>/dev/null; \
	  if [ $$rc = 0 ]; then mv $(TGU)/disk.img $(1) && mv $(TGU)/disk.img.key $(1).key && echo "tg-upgrade: $(1) upgraded"; \
	  else rm -f $(TGU)/disk.img $(TGU)/disk.img.key; echo "tg-upgrade: failed; $(1) is as it was"; false; fi; }
tg-upgrade: tg-image $(STAGEB)/.stamp tests/tg-upgrade.script tests/expand_script.py tgd/tgd/net.py tgd/tgd/telegram-cidr.txt
	@[ -f $(VOL) ] || { echo "tg-upgrade: no volume $(VOL)"; exit 1; }
	$(call tgu_run,$(VOL))

# The architect's path, walked by the GUI runner in run-tg-login's own mode (interactive,
# the same cocoa display) on a copy of the kept volume, with no network
# (tests/tg-login-path.script): a cocoa window shows on the host while it runs. No
# screenshot: the volume holds the account. UPGRADE=1 runs tg-upgrade on the copy first.
TGL_COPY := $(B)/tgl/disk.img
walk-tg-login: tg-image tests/tg-login-path.script
	@[ -f $(TGS_DISK) ] || { echo "walk-tg-login: no volume yet: make run-tg-login (or make tg-install)"; exit 1; }
	@mkdir -p $(B)/tgl && cp $(TGS_DISK) $(TGL_COPY) && cp $(TGS_DISK).key $(TGL_COPY).key
	@if [ -n "$(UPGRADE)" ]; then $(MAKE) --no-print-directory tg-upgrade VOL=$(TGL_COPY) || { rm -f $(TGL_COPY) $(TGL_COPY).key; exit 1; }; fi
	( cd $(ZTGW) && target/debug/zoraya-test-runner --timeout 300 --gui-script $(TG)/tests/tg-login-path.script \
	    --qmp build/tg-login-1.qmp --label tg-login-1 -- \
	    tools/run-qemu.sh --gui --gui-scale 1 --display-opts $(TGS_DISPLAY) --mem $(TGS_MEM) --qmp build/tg-login-1.qmp \
	    --disk $(TGL_COPY) --accel hvf build/zoraya-kernel.elf ); rc=$$?; \
	  cp $(ZTGW)/target/test-logs/tg-login-1.log $(LOGS)/tg-login-1.log 2>/dev/null; rm -f $(TGL_COPY) $(TGL_COPY).key; \
	  [ $$rc = 0 ]

clean:
	rm -rf $(B)/work $(B)/host $(B)/td-zoraya $(B)/python-* $(B)/rootfs-python $(B)/obj $(B)/smoke $(OUT)
