def escapeJson(value: String): String = value
  .replace("\\", "\\\\")
  .replace("\"", "\\\"")
  .replace("\n", "\\n")
  .replace("\r", "\\r")
  .replace("\t", "\\t")

@main def exec(target: String) = {
  val cpg = importCode(target)
  val names = cpg.method
    .filter(node => node.filename != "<includes>" && node.lineNumber.isDefined)
    .filter(node => node.body.typeFullName != "<empty>")
    .filter(node => node.name != "<global>")
    .name
    .l
    .distinct
  println("CINDERGRAPH_JOERN_RESULT_START")
  println(names.map(name => "\"" + escapeJson(name) + "\"").mkString("{\"functions\":[", ",", "]}"))
  println("CINDERGRAPH_JOERN_RESULT_END")
}
