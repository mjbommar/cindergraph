import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

import org.eclipse.cdt.core.dom.ast.ASTVisitor;
import org.eclipse.cdt.core.dom.ast.IASTDeclaration;
import org.eclipse.cdt.core.dom.ast.IASTFunctionDefinition;
import org.eclipse.cdt.core.dom.ast.IASTProblem;
import org.eclipse.cdt.core.dom.ast.IASTTranslationUnit;
import org.eclipse.cdt.core.dom.ast.gnu.c.GCCLanguage;
import org.eclipse.cdt.core.model.ILanguage;
import org.eclipse.cdt.core.parser.DefaultLogService;
import org.eclipse.cdt.core.parser.FileContent;
import org.eclipse.cdt.core.parser.IncludeFileContentProvider;
import org.eclipse.cdt.core.parser.ScannerInfo;

/** Minimal process adapter for the Cindergraph robustness contract. */
public final class CindergraphCdtAdapter {
    private CindergraphCdtAdapter() {}

    private static String json(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '\\' -> out.append("\\\\");
                case '"' -> out.append("\\\"");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) out.append(String.format("\\u%04x", (int) ch));
                    else out.append(ch);
                }
            }
        }
        return out.append('"').toString();
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("usage: adapter INPUT.c");
        Path path = Path.of(args[0]);
        char[] source = Files.readString(path).toCharArray();
        IASTTranslationUnit ast = GCCLanguage.getDefault().getASTTranslationUnit(
            FileContent.create(path.toString(), source),
            new ScannerInfo(),
            IncludeFileContentProvider.getEmptyFilesProvider(),
            null,
            ILanguage.OPTION_IS_SOURCE_UNIT,
            new DefaultLogService()
        );
        List<String> functions = new ArrayList<>();
        int[] problems = {ast.getPreprocessorProblems().length};
        ast.accept(new ASTVisitor() {
            {
                shouldVisitDeclarations = true;
                shouldVisitProblems = true;
            }
            @Override public int visit(IASTDeclaration declaration) {
                if (declaration instanceof IASTFunctionDefinition function) {
                    functions.add(function.getDeclarator().getName().toString());
                }
                return PROCESS_CONTINUE;
            }
            @Override public int visit(IASTProblem problem) {
                problems[0]++;
                return PROCESS_CONTINUE;
            }
        });
        String names = functions.stream().map(CindergraphCdtAdapter::json)
            .reduce((left, right) -> left + "," + right).orElse("");
        System.out.println("{\"functions\":[" + names + "],\"problems\":" + problems[0] + "}");
    }
}
