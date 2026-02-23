// Basic asm block with PushInt and TypeOf
asm {
    __emit("PushInt", 42);
    __emit("TypeOf");
};
