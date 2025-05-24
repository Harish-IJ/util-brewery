import { type Request, type Response, type NextFunction } from "express";

/**
 * Express error handling middleware.
 *
 * This middleware function captures any errors that occur during request
 * processing and sends an appropriate HTTP response to the client. It logs
 * the error details to the console and responds with a JSON object containing
 * the error message and a success flag set to false.
 *
 * @param err - The error object, potentially containing a status code and message.
 * @param _req - The Express request object (unused).
 * @param res - The Express response object used to send the error response.
 * @param _next - The Express next middleware function (unused).
 */

export const errorHandler = (
  err: any,
  _req: Request,
  res: Response,
  _next: NextFunction
) => {
  console.error(err);

  const statusCode = err.statusCode ?? 500;
  const message = err.message ?? "Internal Server Error";

  res.status(statusCode).json({
    success: false,
    message,
  });
};

export default errorHandler;
