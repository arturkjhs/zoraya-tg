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
# ZORAYA is the Zoraya checkout used as toolchain and image, read-only but for
# its build outputs; it must have run `make fetch-ports` and
# `make -f ports/Makefile cxx zlib openssl python`.
ZORAYA ?= $(HOME)/workspace/zoraya-main
TG := $(abspath .)
B := $(TG)/build
OUT := $(TG)/out
LOGS := $(B)/logs
JOBS ?= 2
LOCK := ports/sources.lock

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

.PHONY: fetch td python root smoke clean
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
	@# One archive of them all, for a size and for a simple link.
	rm -f $(OUT)/lib/libtdjson_all.a && cd $(B)/obj && rm -rf tdall && mkdir tdall && cd tdall && \
	  for l in $(TD_LIBS); do mkdir $$l && (cd $$l && $(ZAR) x $(OUT)/lib/lib$$l.a) || exit 1; done && \
	  $(ZAR) rcs $(OUT)/lib/libtdjson_all.a */*.o
	@echo "td: libtdjson_all.a $$(du -k $(OUT)/lib/libtdjson_all.a | cut -f1) KiB"
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
$(PY_STAMP): $(PY_ROOT)/.stamp $(TD_STAMP) $(OUT)/lib/libtdshim.a modules/_tdjson.c
	printf '%s\n' '# zoraya-tg: TDLib JSON client, built in (Makefile, make python)' '*static*' \
	  '_tdjson $(TG)/modules/_tdjson.c -I$(OUT)/include -DTDJSON_STATIC_DEFINE $(OUT)/lib/libtdjson_all.a $(OUT)/lib/libtdshim.a' \
	  > $(PY_BUILD)/Modules/Setup.local
	@$(call logged,PATH=$(ZTOOLS):$$PATH $(MAKE) -C $(PY_BUILD) python LINKCC=$(ZCXX) -j3,python-tdjson)
	$(ZORAYA)/target/debug/zoraya-elf-check $(PY_BUILD)/python
	@touch $@

# ---------------------------------------------------------------- the image part

root: $(OUT)/root/.stamp
$(OUT)/root/.stamp: $(PY_STAMP) $(wildcard tests/*.py)
	rm -rf $(OUT)/root && mkdir -p $(OUT)/root/bin $(OUT)/root/usr/lib $(OUT)/root/home
	cp $(PY_BUILD)/python $(OUT)/root/bin/python3
	cp -R $(PY_ROOT)/usr/lib/. $(OUT)/root/usr/lib/
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

clean:
	rm -rf $(B)/work $(B)/host $(B)/td-zoraya $(B)/python-* $(B)/rootfs-python $(B)/obj $(B)/smoke $(OUT)
