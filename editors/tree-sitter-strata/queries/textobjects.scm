(function_declaration body: (_) @function.inside) @function.around
(law_declaration body: (_) @function.inside) @function.around
(lambda_expression body: (_) @function.inside) @function.around

(type_declaration) @class.around
(trait_declaration body: (_) @class.inside) @class.around
(impl_declaration body: (_) @class.inside) @class.around

(parameters (parameter) @parameter.inside)
(lambda_parameters (lambda_parameter) @parameter.inside)
(arguments (_) @parameter.inside)

(comment) @comment.inside
(comment)+ @comment.around
(doc_comment)+ @comment.around

(test_declaration body: (_) @test.inside) @test.around
