# Read after CPython's Makefile in the python build directory (make tgd-elf): the program
# /bin/tgd linked from the same objects, modules and libraries as python3, with
# tgd/tgd_main.c in place of Programs/python.o. Same compiler flags, same link line.
tgd-elf: Programs/tgd_main.o $(LINK_PYTHON_DEPS)
	$(LINKCC) $(PY_CORE_EXE_LDFLAGS) $(LINKFORSHARED) -o tgd.elf Programs/tgd_main.o $(LINK_PYTHON_OBJS) $(LIBS) $(MODLIBS) $(SYSLIBS)
Programs/tgd_main.o: $(TGD_MAIN) $(PYTHON_HEADERS)
	$(CC) -c $(PY_CORE_CFLAGS) -o $@ $(TGD_MAIN)
