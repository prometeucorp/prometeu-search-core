// Patterns and token lists from pinned upstream c-cpp.ts. MIT.
pub(super) const CPP_CLASS_HEAD_MACROS_RE: &str = r##"\b(?:class|struct)\s+((?:[A-Z][A-Z0-9_]+(?:\s*\((?:[^()]|\([^()]*\))*\))?\s+)+)(?!final\b)[A-Za-z_]\w*(?:\s*::\s*[A-Za-z_]\w*)*"##;
pub(super) const CPP_COMMENT_OR_LITERAL_RE: &str = r##"\/\/(?:\\\r?\n|[^\r\n])*|\/\*[\s\S]*?(?:\*\/|$)|"(?:\\[\s\S]|[^"\\])*(?:"|$)|(?<!\w)(?:u8|[LuU])?'(?:\\[\s\S]|[^'\\])*(?:'|$)"##;
pub(super) const CPP_RUN_TOKEN_RE: &str =
    r##"[A-Z][A-Z0-9_]*(?:\s*\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\))?"##;
pub(super) const CPP_POINTER_ANNOTATION_RE: &str = r##"([*&])\s*((?:[A-Z][A-Z0-9]*_[A-Z0-9_]*(?:\s+|(?=\*)))+)(?=\*|::|(?!(?:and|and_eq|bitand|bitor|compl|not|not_eq|or|or_eq|xor|xor_eq)\b)[A-Za-z_~])"##;
pub(super) const CPP_PAREN_KEYWORDS: &[&str] = &[
    "if",
    "while",
    "for",
    "switch",
    "catch",
    "return",
    "sizeof",
    "alignof",
    "alignas",
    "decltype",
    "typeid",
    "noexcept",
    "throw",
    "static_assert",
    "new",
    "delete",
    "case",
    "requires",
    "co_await",
    "co_yield",
    "co_return",
    "asm",
    "__asm__",
    "__attribute__",
    "__attribute",
    "__declspec",
    "_Pragma",
    "else",
    "do",
    "constexpr",
    "consteval",
    "explicit",
    "typeof",
    "__typeof__",
    "__typeof",
    "_Alignof",
    "__alignof__",
    "volatile",
    "__volatile__",
];
pub(super) const CPP_TRAILING_ATTR_CANDIDATE_RE: &str = r##"\)((?:\s*(?:const|volatile|noexcept|override|final|&&|&)(?![\w]))*)\s*(?=[A-Z][A-Z0-9_]{2,}\b)"##;
pub(super) const CPP_TRAILING_ATTR_RUN_RE: &str =
    r##"(?:[A-Z][A-Z0-9_]{2,}\b(?:\s*\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\))?\s*)+"##;
pub(super) const CPP_TRAILING_ATTR_FOLLOW_RE: &str =
    r##"(?:\{|;|=(?!=)|:(?!:)|->|(?:override|final|const|noexcept)\b)"##;
pub(super) const CPP_TYPE_KEYWORDS: &[&str] = &[
    "void", "bool", "char", "char8_t", "char16_t", "char32_t", "wchar_t", "short", "int", "long",
    "float", "double", "signed", "unsigned", "auto",
];
pub(super) const CPP_KEYWORDS: &[&str] = &[
    "void",
    "bool",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "wchar_t",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "auto",
    "if",
    "while",
    "for",
    "switch",
    "catch",
    "return",
    "sizeof",
    "alignof",
    "alignas",
    "decltype",
    "typeid",
    "noexcept",
    "throw",
    "static_assert",
    "new",
    "delete",
    "case",
    "requires",
    "co_await",
    "co_yield",
    "co_return",
    "asm",
    "__asm__",
    "__attribute__",
    "__attribute",
    "__declspec",
    "_Pragma",
    "else",
    "do",
    "constexpr",
    "consteval",
    "explicit",
    "typeof",
    "__typeof__",
    "__typeof",
    "_Alignof",
    "__alignof__",
    "volatile",
    "__volatile__",
    "const",
    "static",
    "extern",
    "inline",
    "virtual",
    "constinit",
    "friend",
    "mutable",
    "thread_local",
    "register",
    "typedef",
    "using",
    "template",
    "typename",
    "class",
    "struct",
    "union",
    "enum",
    "namespace",
    "operator",
    "goto",
    "break",
    "continue",
    "default",
    "public",
    "private",
    "protected",
    "this",
    "true",
    "false",
    "nullptr",
    "and",
    "or",
    "not",
    "xor",
    "bitand",
    "bitor",
    "compl",
    "and_eq",
    "or_eq",
    "xor_eq",
    "not_eq",
];
pub(super) const CPP_DECLARATOR_ATTR_CANDIDATE_RE: &str =
    r##"(?<![\w$])[A-Z][A-Z0-9]*_[A-Z0-9_]*\b"##;
pub(super) const CPP_DECLARATOR_ATTR_RUN_RE: &str =
    r##"(?:[A-Z][A-Z0-9]*_[A-Z0-9_]*\b(?:\s*\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\))?\s*)+"##;
pub(super) const CPP_LEADING_ATTR_RE: &str = r##"(?m)(^[ \t]*|\]\][ \t]*)((?:[A-Z][A-Z0-9]*_[A-Z0-9_]*\s+)+)(?=(?:static|inline|virtual|explicit|constexpr|consteval|constinit|friend|extern|thread_local|mutable|typedef|using|template|typename|class|struct|union|enum|void|bool|char|char8_t|char16_t|char32_t|wchar_t|short|int|long|float|double|signed|unsigned|auto|decltype|return)\b|\[\[|(?:[A-Za-z_]\w*\s*::\s*)*([A-Za-z_]\w*)\s*::\s*~?\3\s*\(|(?:(?:const|volatile)\s+)*(?:::\s*)?(?!(?:const|volatile|operator|return)\b)[A-Za-z_]\w*(?:\s*<(?:[^<>;{}]|<(?:[^<>;{}]|<[^<>;{}]*>)*>)*>)?(?:\s*::\s*[A-Za-z_]\w*(?:\s*<(?:[^<>;{}]|<(?:[^<>;{}]|<[^<>;{}]*>)*>)*>)?)*(?:\s*(?:const|volatile)\b)?(?:\s*[*&]+\s*|\s+)(?:(?:const|volatile)\s+)?(?:(?:::\s*)?~?[A-Za-z_]\w*(?:\s*::\s*~?[A-Za-z_]\w*)*\s*[(;=[{]|operator\b))"##;
pub(super) const CPP_INLINE_MACRO_RE: &str = r##"\b(ABSL_ATTRIBUTE_ALWAYS_INLINE|LLVM_ATTRIBUTE_ALWAYS_INLINE|JSON_HEDLEY_ALWAYS_INLINE|JSON_HEDLEY_NEVER_INLINE|ABSL_ATTRIBUTE_NOINLINE|LLVM_ATTRIBUTE_NOINLINE|FORCEINLINE_DEBUGGABLE|PROTOBUF_ALWAYS_INLINE|RAPIDJSON_FORCEINLINE|HEDLEY_ALWAYS_INLINE|FOLLY_ALWAYS_INLINE|EIGEN_STRONG_INLINE|EIGEN_ALWAYS_INLINE|HEDLEY_NEVER_INLINE|PUGI__FN_NO_INLINE|GLM_FUNC_QUALIFIER|BOOST_FORCEINLINE|EIGEN_DEVICE_FUNC|MOZ_ALWAYS_INLINE|PROTOBUF_NOINLINE|SIMD_FORCE_INLINE|STDMETHODCALLTYPE|PUGIXML_FUNCTION|MOZ_NEVER_INLINE|SK_ALWAYS_INLINE|CV_ALWAYS_INLINE|_ALWAYS_INLINE_|Q_ALWAYS_INLINE|FMT_CONSTEXPR20|EA_FORCE_INLINE|_FORCE_INLINE_|BOOST_NOINLINE|FOLLY_NOINLINE|SQLITE_PRIVATE|FORCENOINLINE|FMT_CONSTEXPR|GLM_FUNC_DECL|GLM_CONSTEXPR|G_INLINE_FUNC|ALWAYS_INLINE|Q_SCRIPTABLE|NEVER_INLINE|FORCE_INLINE|FORCEINLINE|Q_INVOKABLE|V8_NOINLINE|EA_NOINLINE|FMT_INLINE|GLM_INLINE|SQLITE_API|V8_INLINE|CV_INLINE|CC_INLINE|PUGI__FN|Q_SIGNAL|APIENTRY|NOINLINE|WINAPIV|Q_SLOT|WINAPI)\b(?=\s+[A-Za-z_])"##;
pub(super) const METAL_ATTRIBUTE_RE: &str = r##"\[\[\s*[A-Za-z_]\w*(?:\s*\([^()\n]*\))?(?:\s*,\s*[A-Za-z_]\w*(?:\s*\([^()\n]*\))?)*\s*\]\]"##;
pub(super) const C_RETURN_TYPE_ARG_RE: &str = r##"^\s*[A-Za-z_][\w\s*]*$"##;
pub(super) const C_DECLARATOR_HEAD_RE: &str = r##"[A-Za-z_]\w*\s*\("##;
pub(super) const C_DECLARATOR_TAIL_RE: &str = r##"\s*(?:(?:__attribute__\s*\(\((?:[^()]|\([^()]*\))*\)\)|[A-Z][A-Z0-9_]*(?![\w$])(?:\s*\((?:[^()]|\([^()]*\))*\))?)\s*)*[{;]"##;
pub(super) const LONE_MACRO_LINE_RE: &str =
    r##"^[ \t]*([A-Z][A-Z0-9_]{3,})[ \t]*(?:\/\/[^\n\r]*|\/\*[^\n\r]*\*\/[ \t]*)?\r?$"##;
pub(super) const LONE_MACRO_CONTINUATION_END_RE: &str = r##"[=+\-*/%&|^<>?:,(\\]$"##;
pub(super) const CPP_API_PREFIX_RE: &str =
    r##"\b[A-Z][A-Z0-9_]*(?:_API|_EXPORT|_ABI)\b(?=\s+[A-Za-z_])"##;
pub(super) const CPP_INLINE_ANNOTATION_RE: &str = r##"\b(?:UMETA|UPARAM|UE_DEPRECATED\w*)\s*\("##;
pub(super) const CUDA_LAUNCH_BOUNDS_RE: &str = r##"\b__launch_bounds__\s*\([^()\n]*\)"##;
pub(super) const CUDA_SPECIFIER_RE: &str = r##"\b__(?:global|device|host|constant|shared|managed|grid_constant|forceinline|noinline|launch_bounds)__\b"##;
pub(super) const CUDA_LAUNCH_CONFIG_RE: &str = r##"<<<[^;]{0,400}?>>>"##;
pub(super) const ACCESS_MACRO_LINE: &str =
    r##"(?m)^([ \t]*)([A-Z][A-Z0-9]*_[A-Z0-9_]*)([ \t]*):(?!:)(?=[ \t]*(?:\/\/[^\n]*)?\r?$)"##;
pub(super) const FOUNDATION_ENUM_MACRO_RE: &str = r##"\b(?:(typedef)(\s+))?(NS_ENUM|NS_OPTIONS|NS_CLOSED_ENUM|NS_ERROR_ENUM|CF_ENUM|CF_OPTIONS|CF_CLOSED_ENUM)(\s*)\(([^(),]*),(\s*)([A-Za-z_]\w*)(\s*)\)"##;
pub(super) const C_LEADING_ATTR_MACRO_RE: &str =
    r##"(?m)^([ \t]*)([A-Z][A-Z0-9_]{2,})(?=\s+[A-Za-z_]\w*[\s*]+[A-Za-z_]\w*\s*\()"##;
pub(super) const C_CPLUSPLUS_GUARD_OPEN_RE: &str = r##"^[ \t]*#[ \t]*(?:ifdef[ \t]+__cplusplus\b|if[ \t]+defined[ \t]*\(?[ \t]*__cplusplus[ \t]*\)?)"##;
pub(super) const C_PREPROC_DIRECTIVE_RE: &str = r##"^[ \t]*#"##;
pub(super) const C_PREPROC_ENDIF_RE: &str = r##"^[ \t]*#[ \t]*endif\b"##;
pub(super) const C_STMT_MACRO_KEYWORDS: &[&str] = &[
    "if", "while", "for", "switch", "return", "do", "else", "sizeof",
];
pub(super) const C_SANDWICH_RE: &str = r##"\b(static|extern|inline)([ \t]+)(noinline_for_stack|nokprobe_inline|noinline|notrace|noinstr)\b(?=[ \t]+[A-Za-z_])"##;
pub(super) const C_AUTO_INFER_RE: &str = r##"\bauto(?=[ \t]+[A-Za-z_]\w*[ \t]*=)"##;
pub(super) const C_TRAILING_PARAM_ATTR_RE: &str =
    r##"\b([A-Za-z_]\w*)([ \t]+)([A-Z][A-Z0-9_]{2,})(?=[ \t]*[,)])"##;
pub(super) const C_KERNEL_ANNOTATION_RE: &str = r##"\b(____cacheline_internodealigned_in_smp|__cacheline_internodealigned_in_smp|____cacheline_aligned_in_smp|__cacheline_aligned_in_smp|__no_randomize_layout|____cacheline_aligned|__initdata_or_meminfo|__cacheline_aligned|__initdata_memblock|__randomize_layout|__function_aligned|__init_or_module|__latent_entropy|__always_unused|__ro_after_init|__always_inline|__net_initdata|__maybe_unused|__meminitdata|__read_mostly|__must_check|__deprecated|__nosavedata|__initconst|__bpf_kfunc|__initdata|__exitdata|__net_init|__net_exit|__noreturn|__lockfunc|__no_kcsan|__devinit|__devexit|__cpuinit|__meminit|__visible|__refdata|__noclone|__private|__bitwise|__cpuidle|__kernel|__percpu|__nocast|__malloc|__iomem|__force|__sched|__init|__exit|__user|__used|__cold|__weak|__pure|__ksym|__rcu|__hot|__ref)\b(?!\s*\()"##;
pub(super) const C_PARAM_ANNOTATION_RE: &str = r##"\b(__assume_aligned|__counted_by_le|__counted_by_be|__pt_guarded_by|__counted_by|__guarded_by|__bpf_md_ptr|__must_hold|__acquires|__releases|__cleanup|__aligned|__section|__printf|__scanf|__free)[ \t]*\((?:[^()]|\([^()]*\))*\)"##;
pub(super) const C_TYPE_ARG_HEAD_EXCLUSIONS: &[&str] = &[
    "sizeof",
    "alignof",
    "_Alignof",
    "typeof",
    "__typeof__",
    "__typeof",
    "offsetof",
    "_Generic",
    "va_arg",
    "if",
    "while",
    "for",
    "switch",
    "case",
];
pub(super) const C_TYPE_ARG_HEAD_RE: &str = r##"\b([A-Za-z_]\w*)[ \t]*\("##;
pub(super) const C_TYPE_ARG_OPENER_RE: &str =
    r##"^(struct|union|enum)([ \t\r\n]+)([A-Za-z_]\w*)([ \t\r\n]*\*+)?(?=[ \t\r\n]*[,)])"##;
pub(super) const C_PREFIXED_DECL_MACRO_RE: &str =
    r##"^[ \t]*(?:static|extern)[ \t]+[A-Z][A-Z0-9_]{2,}[ \t]*\("##;
pub(super) const C_PREFIXED_DECL_MACRO_INIT_RE: &str = r##"^([ \t]*)static[ \t]+[A-Z][A-Z0-9_]{2,}[ \t]*\(([^();'"]*?),[ \t]*([A-Za-z_]\w*)[ \t]*\)([ \t]*=[ \t]*\{?[ \t]*\r?)$"##;
pub(super) const C_VA_ARG_RE: &str = r##"\bva_arg[ \t]*\(([^(),]+)(,[^()]*)\)"##;
pub(super) const C_NAMED_VARIADIC_DEFINE_RE: &str =
    r##"^([ \t]*#[ \t]*define[ \t]+[A-Za-z_]\w*\([^()\n]*?\b\w+)\.\.\."##;
pub(super) const C_COND_OPEN_RE: &str = r##"^[ \t]*#[ \t]*(if|ifdef|ifndef)\b[ \t]*(.*)$"##;
pub(super) const C_COND_NEXT_RE: &str = r##"^[ \t]*#[ \t]*(elif|else)\b"##;
pub(super) const C_COND_ENDIF_RE: &str = r##"^[ \t]*#[ \t]*endif\b"##;
